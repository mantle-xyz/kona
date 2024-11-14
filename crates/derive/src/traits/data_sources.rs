//! Contains traits that describe the functionality of various data sources used in the derivation
//! pipeline's stages.

use crate::{errors::PipelineResult, sources::IndexedBlobHash};
use alloc::{boxed::Box, fmt::Debug, vec::Vec};
use alloy_eips::eip4844::Blob;
use alloy_primitives::Bytes;
use async_trait::async_trait;
use core::fmt::Display;
use op_alloy_protocol::BlockInfo;

/// The BlobProvider trait specifies the functionality of a data source that can provide blobs.
#[async_trait]
pub trait BlobProvider {
    /// The error type for the [BlobProvider].
    type Error: Display;

    /// Fetches blobs for a given block ref and the blob hashes.
    async fn get_blobs(
        &mut self,
        block_ref: &BlockInfo,
        blob_hashes: &[IndexedBlobHash],
    ) -> Result<Vec<Box<Blob>>, Self::Error>;
}

/// The EigenDA provider trait specifies the functionality of a data source that can provide EigenDA data
#[async_trait]
pub trait EigenDAProvider {
    /// The error type for the [EigenDAProvider]
    type Error: Display;

    /// Fetches EigenDA data for a given batch_header_hash and blob_index and commitment
    async fn retrieve_blob(
        &mut self,
        batch_header_hash: &[u8],
        blob_index: u32,
    ) -> Result<Vec<u8>, Self::Error>;

    /// Fetches EigenDA data for a given commitment
    async fn retrieve_blob_with_commitment(
        &mut self,
        commitment: &[u8],
    ) -> Result<Vec<u8>, Self::Error>;

    /// Fetches EigenDA data from mantle eigen_da indexer with a given tx_hash
    async fn retrieval_frames_from_da_indexer(&mut self, tx_hash: &str) -> Result<Vec<u8>, Self::Error>;

    /// Weather use mantle eigen_da indexer service
    async fn da_indexer_enable(&mut self) -> Result<bool, Self::Error>;

}


/// Describes the functionality of a data source that can provide data availability information.
#[async_trait]
pub trait DataAvailabilityProvider {
    /// The item type of the data iterator.
    type Item: Send + Sync + Debug + Into<Bytes>;
    /// An iterator over returned bytes data.
    type DataIter: AsyncIterator<Item = Self::Item> + Send + Debug;

    /// Returns the data availability for the block with the given hash, or an error if the block
    /// does not exist in the data source.
    async fn open_data(&self, block_ref: &BlockInfo) -> PipelineResult<Self::DataIter>;
}

/// A simple asynchronous iterator trait.
/// This should be replaced with the `async-iterator` crate
#[async_trait]
pub trait AsyncIterator {
    /// The item type of the iterator.
    type Item: Send + Sync + Debug + Into<Bytes>;

    /// Returns the next item in the iterator, or [crate::errors::PipelineError::Eof] if the
    /// iterator is exhausted.
    async fn next(&mut self) -> PipelineResult<Self::Item>;
}
