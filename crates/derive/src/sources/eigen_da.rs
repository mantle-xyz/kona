use crate::errors::{
    BlobDecodingError, BlobProviderError, EigenDAProviderError, EigenDAProxyError, PipelineError,
};
use crate::prelude::ChainProvider;
use crate::proto::{calldata_frame, CalldataFrame};
use crate::sources::BlobData;
use crate::traits::{BlobProvider, DataAvailabilityProvider, EigenDAProvider};
use crate::types::PipelineResult;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::ToString;
use alloc::vec::Vec;
use alloy_consensus::{Transaction, TxEip4844Variant, TxEnvelope, TxType};
use alloy_eips::eip4844::IndexedBlobHash;
use alloy_primitives::{hex, Address, Bytes};
use alloy_rlp::Rlp;
use async_trait::async_trait;
use core::ops::Deref;
use op_alloy_protocol::BlockInfo;
use prost::Message;
use rlp::{decode, Decodable, DecoderError};
use tracing::{error, info, warn};

/// Useful to dinstiguish between plain calldata and alt-da blob refs
/// Support seamless migration of existing rollups using ETH DA
pub const DERIVATION_VERSION_EIGEN_DA: u8 = 0xed;

pub struct VecOfBytes(pub Vec<Vec<u8>>);

impl Decodable for VecOfBytes {
    fn decode(rlp: &rlp::Rlp<'_>) -> Result<Self, DecoderError> {
        let inner = rlp.as_list::<Vec<u8>>()?;
        Ok(VecOfBytes(inner))
    }
}

/// A data iterator that reads from eigen da.
#[derive(Debug, Clone)]
pub struct EigenDaSource<F, B, E>
where
    F: ChainProvider + Send,
    B: BlobProvider + Send,
    E: EigenDAProvider + Send,
{
    /// Chain provider.
    pub chain_provider: F,
    /// Fetches blobs.
    pub blob_provider: B,
    /// Fetches eigen da blobs.
    pub eigen_da_provider: E,
    /// The address of the batcher contract.
    pub batcher_address: Address,
    /// The L1 Signer.
    pub signer: Address,
    /// Data.
    pub data: Vec<Bytes>,
    /// Whether the source is open.
    pub open: bool,
}

impl<F, B, E> EigenDaSource<F, B, E>
where
    F: ChainProvider + Send,
    B: BlobProvider + Send,
    E: EigenDAProvider + Send,
{
    /// Creates a new EigenDA blob source.
    pub const fn new(
        chain_provider: F,
        blob_provider: B,
        eigen_da_provider: E,
        batcher_address: Address,
        signer: Address,
    ) -> Self {
        Self {
            chain_provider,
            blob_provider,
            eigen_da_provider,
            batcher_address,
            signer,
            data: Vec::new(),
            open: false,
        }
    }

    async fn data_from_eigen_da(
        &mut self,
        txs: Vec<TxEnvelope>,
    ) -> Result<(Vec<Bytes>, Vec<IndexedBlobHash>), EigenDAProviderError> {
        let mut out: Vec<Bytes> = Vec::new();
        let mut hashes = Vec::new();
        let mut number: u64 = 0;

        for tx in txs {
            let (tx_kind, calldata, blob_hashes) = match &tx {
                TxEnvelope::Legacy(tx) => (tx.tx().to(), tx.tx().input.clone(), None),
                TxEnvelope::Eip2930(tx) => (tx.tx().to(), tx.tx().input.clone(), None),
                TxEnvelope::Eip1559(tx) => (tx.tx().to(), tx.tx().input.clone(), None),
                TxEnvelope::Eip4844(blob_tx_wrapper) => match blob_tx_wrapper.tx() {
                    TxEip4844Variant::TxEip4844(tx) => {
                        (tx.to(), tx.input.clone(), Some(tx.blob_versioned_hashes.clone()))
                    }
                    TxEip4844Variant::TxEip4844WithSidecar(tx) => {
                        let tx = tx.tx();
                        (tx.to(), tx.input.clone(), Some(tx.blob_versioned_hashes.clone()))
                    }
                },
                _ => continue,
            };
            let Some(to) = tx_kind else { continue };
            if to != self.batcher_address {
                number += blob_hashes.map_or(0, |h| h.len() as u64);
                continue;
            }
            if tx.recover_signer().unwrap_or_default() != self.signer {
                number += blob_hashes.map_or(0, |h| h.len() as u64);
                continue;
            }
            if self.eigen_da_provider.da_indexer_enable() {
                error!("eigen_da_provider.da_indexer_enable() not implemented");
                break;
            }

            if calldata.len() == 0 {
                if tx.tx_type() == TxType::Eip4844 {
                    let blob_hashes = if let Some(b) = blob_hashes {
                        b
                    } else {
                        continue;
                    };
                    for blob in blob_hashes {
                        let indexed = IndexedBlobHash { hash: blob, index: number };
                        hashes.push(indexed);
                        number += 1;
                    }
                }
                continue;
            }

            if calldata[0] == DERIVATION_VERSION_EIGEN_DA {
                let blob_data = calldata.slice(1..);
                let calldata_frame: CalldataFrame = CalldataFrame::decode(blob_data)
                    .map_err(|e| EigenDAProviderError::ProtoDecodeError(e.to_string()))?;
                if let Some(value) = calldata_frame.value {
                    match value {
                        calldata_frame::Value::Frame(frame) => out.push(Bytes::from(frame)),
                        calldata_frame::Value::FrameRef(frame_ref) => {
                            if frame_ref.quorum_ids.len() == 0 {
                                warn!(target: "eigen-da-source", "decoded frame ref contains no quorum IDs");
                                continue;
                            }
                            let blob_data = self
                                .eigen_da_provider
                                .retrieve_blob_with_commitment(
                                    &frame_ref.commitment,
                                    frame_ref.blob_length,
                                )
                                .await
                                .map_err(|e| EigenDAProviderError::Status(e.to_string()))?;
                            let blobs = &blob_data[..frame_ref.blob_length as usize];
                            let blob_data: VecOfBytes = decode(blobs)
                                .map_err(|e| EigenDAProviderError::RLPDecodeError(e.to_string()))?;
                            for blob in blob_data.0 {
                                out.push(Bytes::from(blob));
                            }
                        }
                    }
                }
            }
        }
        Ok((out, hashes))
    }

    async fn load_blobs(&mut self, block_ref: &BlockInfo) -> Result<(), EigenDAProviderError> {
        if self.open {
            return Ok(());
        }
        let info = self
            .chain_provider
            .block_info_and_transactions_by_hash(block_ref.hash)
            .await
            .map_err(|e| EigenDAProviderError::Backend(e.to_string()))?;
        let (mut blob_data, blob_hashes) = self.data_from_eigen_da(info.1).await?;
        info!(target: "eigen_da", "loading eigen blobs blob hashes len {}, blob data len {}", blob_hashes.len(), blob_data.len());
        if blob_hashes.len() > 0 {
            let blobs =
                self.blob_provider.get_blobs(block_ref, &blob_hashes).await.map_err(|e| {
                    warn!(target: "eigen-da-source", "Failed to fetch blobs: {e}");
                    EigenDAProviderError::Backend(
                        BlobProviderError::Backend(e.to_string()).to_string(),
                    )
                })?;
            let mut whole_blob_data = Vec::new();
            for blob in blobs {
                if blob.is_empty() {
                    return Err(EigenDAProviderError::RLPDecodeError(
                        BlobDecodingError::MissingData.to_string(),
                    ));
                }
                whole_blob_data.extend(blob.to_vec().clone());
            }
            let rlp_blob: VecOfBytes = decode(&whole_blob_data)
                .map_err(|e| EigenDAProviderError::RetrieveFramesFromDaIndexer(e.to_string()))?;
            for blob in rlp_blob.0 {
                blob_data.push(Bytes::from(blob));
            }
        }
        self.open = true;
        self.data = blob_data;
        Ok(())
    }

    /// Extracts the next data from the source.
    fn next_data(&mut self) -> Result<Bytes, PipelineResult<Bytes>> {
        if self.data.is_empty() {
            return Err(Err(PipelineError::Eof.temp()));
        }

        Ok(self.data.remove(0))
    }
}

#[async_trait]
impl<F, B, E> DataAvailabilityProvider for EigenDaSource<F, B, E>
where
    F: ChainProvider + Send,
    B: BlobProvider + Send,
    E: EigenDAProvider + Send,
{
    type Item = Bytes;

    async fn next(&mut self, block_ref: &BlockInfo) -> PipelineResult<Self::Item> {
        let result = self.load_blobs(block_ref).await;
        match result {
            Ok(_) => (),

            Err(e) => {
                return Err(PipelineError::Provider(format!(
                    "Failed to load eigen_da blobs from stream: {}, err: {}",
                    block_ref.hash,
                    e.to_string()
                ))
                .temp());
            }
        }

        let next_data = match self.next_data() {
            Ok(d) => d,
            Err(e) => return e,
        };
        //TODO EigenDA decode

        Ok(next_data)
    }

    fn clear(&mut self) {
        self.data.clear();
        self.open = false;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::test_utils::TestEigenDaProvider;
    use alloc::vec;
    use alloy_primitives::keccak256;
    use alloy_rlp::Decodable;

    #[tokio::test]
    async fn test_calldata_frame_decode() {
        let txs = valid_eigen_da_txs();
        let mut eigen_da_provider = TestEigenDaProvider::default();
        for tx in txs {
            let (tx_kind, calldata, blob_hashes) = match &tx {
                TxEnvelope::Legacy(tx) => (tx.tx().to(), tx.tx().input.clone(), None),
                TxEnvelope::Eip2930(tx) => (tx.tx().to(), tx.tx().input.clone(), None),
                TxEnvelope::Eip1559(tx) => (tx.tx().to(), tx.tx().input.clone(), None),
                TxEnvelope::Eip4844(blob_tx_wrapper) => match blob_tx_wrapper.tx() {
                    TxEip4844Variant::TxEip4844(tx) => {
                        (tx.to(), tx.input.clone(), Some(tx.blob_versioned_hashes.clone()))
                    }
                    TxEip4844Variant::TxEip4844WithSidecar(tx) => {
                        let tx = tx.tx();
                        (tx.to(), tx.input.clone(), Some(tx.blob_versioned_hashes.clone()))
                    }
                },
                _ => continue,
            };
            assert_eq!(calldata[0], DERIVATION_VERSION_EIGEN_DA);

            let blob_data = calldata.slice(1..);
            let calldata_frame: CalldataFrame = CalldataFrame::decode(blob_data).unwrap();
            if let Some(value) = calldata_frame.value {
                match value {
                    calldata_frame::Value::Frame(frame) => {}
                    calldata_frame::Value::FrameRef(frame_ref) => {
                        if frame_ref.quorum_ids.len() == 0 {
                            warn!(target: "eigen-da-source", "decoded frame ref contains no quorum IDs");
                            continue;
                        }
                        let commitment = hex::encode(frame_ref.commitment.as_slice()).to_string();

                        assert_eq!(commitment, "010000f901d8f852f842a00dbbd22149b419a9a751c25065b58745f4216dc3ae4e9ad583306c395387b6a3a02673dfa25dd3095246eeffb639d3e11108a1ba75dd29b86c3a4200ed00210e4e820200cac480213701c401213710f90181830148ae81a5f873eba0c42bcd27bcd22ba55c4189a25d362343838cb75f57979baa0686ec5381a944c3820001826362832a79cba07263089b84cbb2963e4f50a930243c081ab14b01c0c92d57c3029590bd9dfc9200832a7a20a05419bc29ac025512311c14f23d9613e408448e47bb31f71614e1f82b6c63966cb9010074b13a3acaba35d3749063c19806c9a2f2004b318d55edd6cb5129d958807ea7ac09584a2c6ea029ed34c72f849862e4189928e90931e07093209016f5fc70a6c4a8c3237c25c4f236bb25c105fd7dbd6e4a00153c69c0757d8cbf02f966167ccae243412c20de1c3a38a50818dc7f9f3e02dcb3bc4e54800f2224b8c1eaa9955e41792fa0e401f2814ee209331126149c630c34e1b8e2f804955582022676e232d24d7784b496fc997d98db2849b1bfa8443b362723fc603da8de11704a1ef50414e11234496cfac67aebdd2faa24840ffe7f04506652b8a11a534b024a40bc7e99fee042336f425eb16e40e4267593415860204c9069723dbaca8cf2e596dc820001".to_string());
                        let blob_data = eigen_da_provider
                            .retrieve_blob_with_commitment(
                                &frame_ref.commitment,
                                frame_ref.blob_length,
                            )
                            .await
                            .map_err(|e| EigenDAProviderError::Status(e.to_string()))
                            .unwrap();
                        let blobs = &blob_data[..frame_ref.blob_length as usize];
                        let blob_data: VecOfBytes = decode(blobs)
                            .map_err(|e| EigenDAProviderError::RLPDecodeError(e.to_string()))
                            .unwrap();
                    }
                }
            }
        }
    }

    pub(crate) fn valid_eigen_da_txs() -> Vec<TxEnvelope> {
        // https://sepolia.etherscan.io/getRawTx?tx=0xfd10d26ace7eec30487bdad54ef5348dfdff48061129cf6e2adf6182a950d5a9
        let raw_tx =
            alloy_primitives::hex::decode("0x02f9026483aa36a7830107f1830f424085083f58abbe8271f49454da4d1124b2310757562b8ee9cea69b25bb46a180b901f2ed12ee0318cbf3a9012202000128a15baa06de03010000f901d8f852f842a00dbbd22149b419a9a751c25065b58745f4216dc3ae4e9ad583306c395387b6a3a02673dfa25dd3095246eeffb639d3e11108a1ba75dd29b86c3a4200ed00210e4e820200cac480213701c401213710f90181830148ae81a5f873eba0c42bcd27bcd22ba55c4189a25d362343838cb75f57979baa0686ec5381a944c3820001826362832a79cba07263089b84cbb2963e4f50a930243c081ab14b01c0c92d57c3029590bd9dfc9200832a7a20a05419bc29ac025512311c14f23d9613e408448e47bb31f71614e1f82b6c63966cb9010074b13a3acaba35d3749063c19806c9a2f2004b318d55edd6cb5129d958807ea7ac09584a2c6ea029ed34c72f849862e4189928e90931e07093209016f5fc70a6c4a8c3237c25c4f236bb25c105fd7dbd6e4a00153c69c0757d8cbf02f966167ccae243412c20de1c3a38a50818dc7f9f3e02dcb3bc4e54800f2224b8c1eaa9955e41792fa0e401f2814ee209331126149c630c34e1b8e2f804955582022676e232d24d7784b496fc997d98db2849b1bfa8443b362723fc603da8de11704a1ef50414e11234496cfac67aebdd2faa24840ffe7f04506652b8a11a534b024a40bc7e99fee042336f425eb16e40e4267593415860204c9069723dbaca8cf2e596dc820001c001a0f421ccc336435722bdf41ef041a278b0851790dae1946be7033c2e057ffede46a027493ede5c5e490f14fc2b06cc444433b3f2d6e1079451d519311c1f4ab8f4b0").unwrap();
        let eoa = TxEnvelope::decode(&mut raw_tx.as_slice()).unwrap();
        vec![eoa]
    }
}
