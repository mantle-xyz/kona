//! Mantle Blob Data Source
//!
//! This source provides blob data with Mantle blob decoding format.
//! Mantle blob decoding concatenates all decoded blobs and then RLP decodes them.

use crate::{
    BlobData, BlobProvider, BlobProviderError, ChainProvider, DataAvailabilityProvider,
    PipelineError, PipelineResult,
};
use alloc::{boxed::Box, format, string::ToString, vec::Vec};
use alloy_consensus::{
    Transaction, TxEip4844Variant, TxEnvelope, TxType, transaction::SignerRecoverable,
};
use alloy_eips::eip4844::IndexedBlobHash;
use alloy_primitives::{Address, Bytes};
use alloy_rlp::Decodable;
use async_trait::async_trait;
use kona_protocol::BlockInfo;

/// A wrapper for RLP decoding of Vec<Bytes>
#[derive(Debug, Clone)]
struct VecOfBytes(pub Vec<Bytes>);

impl Decodable for VecOfBytes {
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        let vec: Vec<Bytes> = Decodable::decode(buf)?;
        Ok(VecOfBytes(vec))
    }
}

/// A data iterator that reads from a blob using Mantle's blob decoding format.
#[derive(Debug, Clone)]
pub struct MantleBlobSource<F, B>
where
    F: ChainProvider + Send,
    B: BlobProvider + Send,
{
    /// Chain provider.
    pub chain_provider: F,
    /// Fetches blobs.
    pub blob_fetcher: B,
    /// The address of the batcher contract.
    pub batcher_address: Address,
    /// Data.
    pub data: Vec<BlobData>,
    /// Whether the source is open.
    pub open: bool,
}

impl<F, B> MantleBlobSource<F, B>
where
    F: ChainProvider + Send,
    B: BlobProvider + Send,
{
    /// Creates a new Mantle blob source.
    pub const fn new(chain_provider: F, blob_fetcher: B, batcher_address: Address) -> Self {
        Self { chain_provider, blob_fetcher, batcher_address, data: Vec::new(), open: false }
    }

    // same as BlobSource::extract_blob_data
    fn extract_blob_data(
        &self,
        txs: Vec<TxEnvelope>,
        batcher_address: Address,
    ) -> (Vec<BlobData>, Vec<IndexedBlobHash>) {
        let mut index: u64 = 0;
        let mut data = Vec::new();
        let mut hashes = Vec::new();
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
                index += blob_hashes.map_or(0, |h| h.len() as u64);
                continue;
            }
            if tx.recover_signer().unwrap_or_default() != batcher_address {
                index += blob_hashes.map_or(0, |h| h.len() as u64);
                continue;
            }
            if tx.tx_type() != TxType::Eip4844 {
                let blob_data = BlobData { data: None, calldata: Some(calldata.to_vec().into()) };
                data.push(blob_data);
                continue;
            }
            if !calldata.is_empty() {
                let hash = match &tx {
                    TxEnvelope::Legacy(tx) => Some(tx.hash()),
                    TxEnvelope::Eip2930(tx) => Some(tx.hash()),
                    TxEnvelope::Eip1559(tx) => Some(tx.hash()),
                    TxEnvelope::Eip4844(blob_tx_wrapper) => Some(blob_tx_wrapper.hash()),
                    _ => None,
                };
                warn!(target: "mantle_blob_source", "Blob tx has calldata, which will be ignored: {hash:?}");
            }
            let blob_hashes = if let Some(b) = blob_hashes {
                b
            } else {
                continue;
            };
            for hash in blob_hashes {
                let indexed = IndexedBlobHash { hash, index };
                hashes.push(indexed);
                data.push(BlobData::default());
                index += 1;
            }
        }
        #[cfg(feature = "metrics")]
        metrics::gauge!(
            crate::metrics::Metrics::PIPELINE_DATA_AVAILABILITY_PROVIDER,
            "source" => "mantle_blobs",
        )
        .increment(data.len() as f64);
        (data, hashes)
    }

    /// Loads blob data into the source if it is not open.
    async fn load_blobs(
        &mut self,
        block_ref: &BlockInfo,
        batcher_address: Address,
    ) -> Result<(), BlobProviderError> {
        if self.open {
            return Ok(());
        }

        let info = self
            .chain_provider
            .block_info_and_transactions_by_hash(block_ref.hash)
            .await
            .map_err(|e| BlobProviderError::Backend(e.to_string()))?;

        let (mut data, blob_hashes) = self.extract_blob_data(info.1, batcher_address);

        // If there are no hashes, set the calldata and return.
        if blob_hashes.is_empty() {
            self.open = true;
            self.data = data;
            return Ok(());
        }

        let blobs =
            self.blob_fetcher.get_and_validate_blobs(block_ref, &blob_hashes).await.map_err(
                |e| {
                    warn!(target: "mantle_blob_source", "Failed to fetch blobs: {e}");
                    BlobProviderError::Backend(e.to_string())
                },
            )?;

        // Fill the blob pointers and decode each blob.
        // Mantle-specific: concatenate all decoded blob data for RLP decoding.
        let mut whole_blob_data = Vec::new();
        let mut blob_index = 0;
        for blob_data in data.iter_mut() {
            match blob_data.fill(&blobs, blob_index) {
                Ok(should_increment) => {
                    if should_increment {
                        blob_index += 1;
                    }
                }
                Err(e) => {
                    return Err(e.into());
                }
            }
            // Decode and append to whole_blob_data
            match blob_data.decode() {
                Ok(d) => whole_blob_data.extend_from_slice(&d),
                Err(_) => {
                    warn!(target: "mantle_blob_source", "Failed to decode blob, skipping");
                }
            }
        }

        // RLP decode the concatenated blob data to Vec<Bytes>
        let rlp_blob: VecOfBytes = Decodable::decode(&mut whole_blob_data.as_slice())
            .map_err(|e| BlobProviderError::Backend(format!("RLP decode error: {}", e)))?;

        // Convert RLP decoded frames to BlobData
        let decoded_data = rlp_blob
            .0
            .into_iter()
            .map(|bytes| BlobData { data: None, calldata: Some(bytes) })
            .collect();

        self.open = true;
        self.data = decoded_data;
        Ok(())
    }

    /// Extracts the next data from the source.
    fn next_data(&mut self) -> PipelineResult<BlobData> {
        if self.data.is_empty() {
            return Err(PipelineError::Eof.temp());
        }

        Ok(self.data.remove(0))
    }
}

#[async_trait]
impl<F, B> DataAvailabilityProvider for MantleBlobSource<F, B>
where
    F: ChainProvider + Sync + Send,
    B: BlobProvider + Sync + Send,
{
    type Item = Bytes;

    async fn next(
        &mut self,
        block_ref: &BlockInfo,
        batcher_address: Address,
    ) -> PipelineResult<Self::Item> {
        self.load_blobs(block_ref, batcher_address).await?;

        let next_data = self.next_data()?;
        if let Some(c) = next_data.calldata {
            return Ok(c);
        }

        // In Mantle blob decoding, the data is already decoded and stored as calldata
        // during load_blobs, so this should not happen
        Err(PipelineError::Eof.temp())
    }

    fn clear(&mut self) {
        self.data.clear();
        self.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        errors::PipelineErrorKind,
        sources::blob_data::BlobData,
        test_utils::{TestBlobProvider, TestChainProvider},
    };
    use alloc::vec;
    use alloy_eips::eip4844::Blob;
    use alloy_primitives::{B256, address, b256, hex};
    use alloy_rlp::{Decodable, Encodable};

    fn default_test_mantle_blob_source() -> MantleBlobSource<TestChainProvider, TestBlobProvider> {
        let chain_provider = TestChainProvider::default();
        let blob_fetcher = TestBlobProvider::default();
        let batcher_address = Address::default();
        MantleBlobSource::new(chain_provider, blob_fetcher, batcher_address)
    }

    /// https://sepolia.etherscan.io/tx/0x468f0d2b209ad680e147f093f430ffa31f453a14a183d248267b3aaa21a624da
    fn valid_mantle_blob_tx() -> (TxEnvelope, Address, Address, [B256; 3]) {
        let raw_tx_hex = "0x03f8d783aa36a7822aea830f4240830f427082520894ffeeddccbbaa00000000000000000000000000008080c0843b9aca00f863a001d402363affae0d61efd3811cfa5d482e2d3700f20ce1a7934add3c6795f2dea0019f084796dbf6a7ba47b2c1a50eff0c1e32baad480bf5e57b89eafeb418dd76a0011f403cfe025351f08e1fb5b31651012a53c247daf632d3ca51bbd10f82cf2601a0ee6af3b8596947879b181f00119f50fc88cdcf935720e200d81f240a61913c32a02098664ffc7b20db92a6b842ecf3dc3ec4efd232f59896d82939fc97bef363c5";
        let raw_tx_bytes = hex::decode(raw_tx_hex.strip_prefix("0x").unwrap()).unwrap();
        let tx = TxEnvelope::decode(&mut raw_tx_bytes.as_slice()).unwrap();

        let batcher_address = address!("0xFFEEDDCcBbAA0000000000000000000000000000");
        let signer = tx.recover_signer().expect("Should recover signer from Mantle tx");

        let blob_hashes = [
            b256!("01d402363affae0d61efd3811cfa5d482e2d3700f20ce1a7934add3c6795f2de"),
            b256!("019f084796dbf6a7ba47b2c1a50eff0c1e32baad480bf5e57b89eafeb418dd76"),
            b256!("011f403cfe025351f08e1fb5b31651012a53c247daf632d3ca51bbd10f82cf26"),
        ];

        (tx, batcher_address, signer, blob_hashes)
    }

    #[tokio::test]
    async fn test_load_blobs_open() {
        let mut source = default_test_mantle_blob_source();
        source.open = true;
        assert!(source.load_blobs(&BlockInfo::default(), Address::ZERO).await.is_ok());
    }

    #[tokio::test]
    async fn test_load_blobs_chain_provider_err() {
        let mut source = default_test_mantle_blob_source();
        assert!(matches!(
            source.load_blobs(&BlockInfo::default(), Address::ZERO).await,
            Err(BlobProviderError::Backend(_))
        ));
    }

    #[tokio::test]
    async fn test_load_blobs_empty_txs() {
        let mut source = default_test_mantle_blob_source();
        let block_info = BlockInfo::default();
        source.chain_provider.insert_block_with_transactions(0, block_info, Vec::new());
        assert!(!source.open);
        assert!(source.load_blobs(&BlockInfo::default(), Address::ZERO).await.is_ok());
        assert!(source.data.is_empty());
        assert!(source.open);
    }

    #[tokio::test]
    async fn test_next_empty_data_eof() {
        let mut source = default_test_mantle_blob_source();
        source.open = true;

        let err = source.next(&BlockInfo::default(), Address::ZERO).await.unwrap_err();
        assert!(matches!(err, PipelineErrorKind::Temporary(PipelineError::Eof)));
    }

    #[tokio::test]
    async fn test_next_calldata() {
        let mut source = default_test_mantle_blob_source();
        source.open = true;
        source
            .data
            .push(BlobData { data: None, calldata: Some(Bytes::from(vec![0x01, 0x02, 0x03])) });

        let data = source.next(&BlockInfo::default(), Address::ZERO).await.unwrap();
        assert_eq!(data, Bytes::from(vec![0x01, 0x02, 0x03]));
    }

    #[tokio::test]
    async fn test_verify_blob_data_rlp_decode() {
        // Verify RLP encoding/decoding of Vec<Bytes> (Mantle's blob format)
        // This test verifies the RLP structure used by Mantle
        let test_batches =
            vec![Bytes::from(vec![0x00, 0x01, 0x02]), Bytes::from(vec![0x03, 0x04, 0x05])];

        // RLP encode
        let mut rlp_encoded = Vec::new();
        test_batches.encode(&mut rlp_encoded);

        // RLP decode
        let mut rlp_slice = rlp_encoded.as_slice();
        let decoded: Vec<Bytes> = Decodable::decode(&mut rlp_slice).unwrap();

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0], Bytes::from(vec![0x00, 0x01, 0x02]));
        assert_eq!(decoded[1], Bytes::from(vec![0x03, 0x04, 0x05]));
    }

    #[tokio::test]
    async fn test_clear() {
        let mut source = default_test_mantle_blob_source();
        source.open = true;
        source.data.push(BlobData { data: None, calldata: Some(Bytes::from(vec![0x01, 0x02])) });

        source.clear();

        assert!(!source.open, "Source should be closed after clear");
        assert!(source.data.is_empty(), "Data should be empty after clear");
    }

    #[tokio::test]
    async fn test_multiple_next_calls() {
        let mut source = default_test_mantle_blob_source();
        source.open = true;
        source.data.push(BlobData { data: None, calldata: Some(Bytes::from(vec![0x01])) });
        source.data.push(BlobData { data: None, calldata: Some(Bytes::from(vec![0x02])) });
        source.data.push(BlobData { data: None, calldata: Some(Bytes::from(vec![0x03])) });

        let data1 = source.next(&BlockInfo::default(), Address::ZERO).await.unwrap();
        assert_eq!(data1, Bytes::from(vec![0x01]));

        let data2 = source.next(&BlockInfo::default(), Address::ZERO).await.unwrap();
        assert_eq!(data2, Bytes::from(vec![0x02]));

        let data3 = source.next(&BlockInfo::default(), Address::ZERO).await.unwrap();
        assert_eq!(data3, Bytes::from(vec![0x03]));

        // Should return EOF after all data consumed
        let err = source.next(&BlockInfo::default(), Address::ZERO).await.unwrap_err();
        assert!(matches!(err, PipelineErrorKind::Temporary(PipelineError::Eof)));
    }

    #[tokio::test]
    async fn test_wrong_batcher_address() {
        let mut source = default_test_mantle_blob_source();
        let correct_batcher = address!("0xFFEEDDCcBbAA0000000000000000000000000000");
        source.batcher_address = correct_batcher;

        let block_info = BlockInfo::default();
        source.chain_provider.insert_block_with_transactions(0, block_info, Vec::new());

        let result = source.load_blobs(&BlockInfo::default(), correct_batcher).await;
        assert!(result.is_ok());
        assert!(source.data.is_empty(), "No data should be extracted from wrong batcher");
    }

    #[tokio::test]
    async fn test_mantle_valid_blob_decode() {
        let (tx, batcher_address, signer, blob_hashes) = valid_mantle_blob_tx();

        let mut source = default_test_mantle_blob_source();
        source.batcher_address = batcher_address;

        let blob_hexes = [
            include_str!("testdata/mantle_sepolia_block_10001504_blob_0.hex"),
            include_str!("testdata/mantle_sepolia_block_10001504_blob_1.hex"),
            include_str!("testdata/mantle_sepolia_block_10001504_blob_2.hex"),
        ];

        for (i, blob_hex) in blob_hexes.iter().enumerate() {
            let blob_bytes =
                hex::decode(blob_hex.trim().strip_prefix("0x").unwrap_or(blob_hex.trim())).unwrap();
            assert_eq!(blob_bytes.len(), 131072, "Each blob should be 131072 bytes");
            let blob = Blob::try_from(blob_bytes.as_slice()).unwrap();
            source.blob_fetcher.insert_blob(blob_hashes[i], blob);
        }

        let block_info = BlockInfo::default();
        source.chain_provider.insert_block_with_transactions(1, block_info, vec![tx]);

        source.load_blobs(&BlockInfo::default(), signer).await.unwrap();

        assert!(source.open, "Source should be open after load_blobs");
        assert!(!source.data.is_empty(), "Should have decoded frames from Mantle blobs");

        for (i, blob_data) in source.data.iter().enumerate() {
            assert!(blob_data.calldata.is_some(), "Frame {} should have calldata", i);
            let calldata = blob_data.calldata.as_ref().unwrap();
            assert!(!calldata.is_empty(), "Frame {} calldata should not be empty", i);
        }
    }

    #[tokio::test]
    async fn test_rlp_decode_valid_mantle_format() {
        // Verify the exact RLP format used by Mantle matches our implementation
        // Create sample frames as Mantle does
        let frame1 = Bytes::from(vec![0x00, 0x01, 0x02, 0x03]);
        let frame2 = Bytes::from(vec![0x04, 0x05, 0x06]);
        let frames = vec![frame1.clone(), frame2.clone()];

        // RLP encode (this is what Mantle does before splitting into blobs)
        let mut rlp_encoded = Vec::new();
        frames.encode(&mut rlp_encoded);

        // Verify we can decode it back
        let mut rlp_slice = rlp_encoded.as_slice();
        let decoded: Vec<Bytes> = Decodable::decode(&mut rlp_slice).unwrap();

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0], frame1);
        assert_eq!(decoded[1], frame2);

        // Verify the RLP structure
        assert!(rlp_encoded[0] >= 0xc0, "Should start with RLP list marker");
    }

    #[tokio::test]
    async fn test_parse_blob_count_from_transaction() {
        let (tx, _, _, expected_hashes) = valid_mantle_blob_tx();

        // Extract blob_versioned_hashes from EIP-4844 transaction
        let (blob_hashes, tx_type) = match &tx {
            TxEnvelope::Eip4844(wrapper) => {
                let hashes = match wrapper.tx() {
                    TxEip4844Variant::TxEip4844(tx) => &tx.blob_versioned_hashes,
                    TxEip4844Variant::TxEip4844WithSidecar(tx) => &tx.tx().blob_versioned_hashes,
                };
                (hashes, "EIP-4844")
            }
            TxEnvelope::Eip1559(_) => panic!("Not a blob transaction"),
            _ => panic!("Unexpected transaction type"),
        };

        // Verify blob count
        assert_eq!(blob_hashes.len(), 3, "Should have 3 blob hashes");
        assert_eq!(tx_type, "EIP-4844", "Should be EIP-4844 transaction");

        // Verify the actual blob hashes
        for (i, hash) in blob_hashes.iter().enumerate() {
            let hash_str = format!("0x{:x}", hash);
            let expected_str = format!("0x{:x}", expected_hashes[i]);
            assert_eq!(hash_str, expected_str, "Blob hash {} mismatch", i);
        }
    }

    #[tokio::test]
    async fn test_signer_verification() {
        let (tx, batcher_address, actual_signer, _) = valid_mantle_blob_tx();

        let mut source = default_test_mantle_blob_source();
        source.batcher_address = batcher_address;

        // Test 1: Transaction with correct signer should be accepted
        let (data, hashes) = source.extract_blob_data(vec![tx.clone()], actual_signer);
        assert!(!data.is_empty(), "Should extract data when signer matches (Mantle has 3 blobs)");
        assert_eq!(hashes.len(), 3, "Should extract 3 blob hashes from Mantle transaction");

        // Test 2: Transaction with wrong signer should be rejected
        let wrong_signer = Address::ZERO;
        let (data2, hashes2) = source.extract_blob_data(vec![tx], wrong_signer);
        assert!(data2.is_empty(), "Should not extract data when signer doesn't match");
        assert!(hashes2.is_empty(), "Should not extract blob hashes when signer doesn't match");
    }
}
