use alloc::format;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::ops::Deref;
use alloy_consensus::{Transaction, TxEip4844Variant, TxEnvelope, TxType};
use alloy_primitives::{hex, Address, Bytes};
use alloy_rlp::Rlp;
use async_trait::async_trait;
use op_alloy_protocol::BlockInfo;
use rlp::{decode, Decodable, DecoderError};
use tracing::warn;
use crate::errors::{BlobDecodingError, BlobProviderError, EigenDAProviderError, EigenDAProxyError, PipelineError, PipelineResult};
use crate::prelude::ChainProvider;
use crate::proto::{calldata_frame, CalldataFrame};
use crate::sources::{BlobData, IndexedBlobHash};
use crate::traits::{AsyncIterator, BlobProvider, EigenDAProvider};
use prost::Message;
use alloc::boxed::Box;



/// Useful to dinstiguish between plain calldata and alt-da blob refs
/// Support seamless migration of existing rollups using ETH DA
pub const DERIVATION_VERSION_EIGEN_DA:u8 = 0xed;


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
    /// Block Ref
    pub block_ref: BlockInfo,
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
        block_ref: BlockInfo,
        signer: Address,
    ) -> Self {
        Self {
            chain_provider,
            blob_provider,
            eigen_da_provider,
            batcher_address,
            block_ref,
            signer,
            data: Vec::new(),
            open: false,
        }
    }

    async fn data_from_eigen_da(&mut self, txs: Vec<TxEnvelope>) -> Result<(Vec<Bytes>, Vec<IndexedBlobHash>),EigenDAProviderError> {
        let mut out:Vec<Bytes> = Vec::new();
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
            let Some(to) = tx_kind.to().copied() else { continue };

            if to != self.batcher_address {
                number += blob_hashes.map_or(0, |h| h.len() as u64);
                continue;
            }
            if tx.recover_signer().unwrap_or_default() != self.signer {
                number += blob_hashes.map_or(0, |h| h.len() as u64);
                continue;
            }
            if self.eigen_da_provider.da_indexer_enable() {
                let data = self.eigen_da_provider
                    .retrieval_frames_from_da_indexer(&*hex::encode(tx.tx_hash())).await.map_err(|e|EigenDAProviderError::String(e.to_string()))?;

                let blob_data:Vec<u8> = decode(&*data).map_err(|e|EigenDAProviderError::RetrieveFramesFromDaIndexer(e.to_string()))?;
                out.push(Bytes::from(blob_data.clone()));
                continue;
            }

            if calldata.len() == 0 {
                if tx.tx_type() == TxType::Eip4844 {
                    let blob_hashes = if let Some(b) = blob_hashes {
                        b
                    } else {
                        continue;
                    };
                    for blob in blob_hashes {
                        let indexed = IndexedBlobHash { hash: blob, index: number as usize };
                        hashes.push(indexed);
                        number += 1;
                    }
                }
                continue;
            }

            if calldata[0] == DERIVATION_VERSION_EIGEN_DA {
                let blob_data = calldata.slice(1..);
                let calldata_frame: CalldataFrame = CalldataFrame::decode(blob_data)
                    .map_err(|e|EigenDAProviderError::ProtoDecodeError(e.to_string()))?;
                if let Some(value) = calldata_frame.value {
                    match value {
                        calldata_frame::Value::Frame(frame) => {
                            out.push(Bytes::from(frame))
                        }
                        calldata_frame::Value::FrameRef(frame_ref) => {
                            if frame_ref.quorum_ids.len() == 0 {
                                warn!(target: "eigen-da-source", "decoded frame ref contains no quorum IDs");
                                continue;
                            }
                            let blob_data = self.eigen_da_provider
                                .retrieve_blob(&*frame_ref.batch_header_hash, frame_ref.blob_index)
                                .await.map_err(|e|EigenDAProviderError::String(e.to_string()))?;
                            let blobs = &blob_data[..frame_ref.blob_length as usize];
                            let blob_data:Vec<u8> = decode(blobs).map_err(|e|EigenDAProviderError::RetrieveFramesFromDaIndexer(e.to_string()))?;
                            out.push(Bytes::from(blob_data.clone()));
                        }
                    }

                }
            }
        }
        Ok((out,hashes))
    }


    async fn load_blobs(&mut self) -> Result<(), EigenDAProviderError> {
        if self.open {
            return Ok(());
        }
        let info = self
            .chain_provider
            .block_info_and_transactions_by_hash(self.block_ref.hash)
            .await
            .map_err(|e| EigenDAProviderError::Backend(e.to_string()))?;
        let (mut blob_data, blob_hashes) = self.data_from_eigen_da(info.1).await?;
        if blob_hashes.len() > 0 {
            let blobs =
                self.blob_provider.get_blobs(&self.block_ref, &blob_hashes).await.map_err(|e| {
                    warn!(target: "blob-source", "Failed to fetch blobs: {e}");
                    EigenDAProviderError::Blob(BlobProviderError::Backend(e.to_string()).to_string())
                })?;
            let mut whole_blob_data = Vec::new();
            for blob in blobs {
                if blob.is_empty() {
                    return Err(EigenDAProviderError::RLPDecodeError(BlobDecodingError::MissingData.to_string()));
                }
                whole_blob_data.extend(blob.to_vec().clone());
            }
            let rlp_blob:Vec<u8> = decode(&whole_blob_data).map_err(|e|EigenDAProviderError::RetrieveFramesFromDaIndexer(e.to_string()))?;
            blob_data.push(Bytes::from(rlp_blob.clone()));
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
impl<F, B, E> AsyncIterator for EigenDaSource<F, B, E>
where
    F: ChainProvider + Send,
    B: BlobProvider + Send,
    E: EigenDAProvider + Send,
{
    type Item = Bytes;

    async fn next(&mut self) -> PipelineResult<Self::Item> {
        if self.load_blobs().await.is_err() {
            return Err(PipelineError::Provider(format!(
                "Failed to load blobs from stream: {}",
                self.block_ref.hash
            ))
                .temp());
        }

        let next_data = match self.next_data() {
            Ok(d) => d,
            Err(e) => return e,
        };

         Ok(next_data)

    }
}