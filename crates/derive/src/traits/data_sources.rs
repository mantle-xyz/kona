//! Contains traits that describe the functionality of various data sources used in the derivation
//! pipeline's stages.

use crate::{errors::PipelineErrorKind, types::PipelineResult};
use alloc::{boxed::Box, fmt::Debug, string::ToString, vec::Vec};
use alloy_eips::eip4844::{Blob, IndexedBlobHash};
use alloy_primitives::Bytes;
use async_trait::async_trait;
use core::fmt::Display;
use op_alloy_protocol::BlockInfo;

/// The BlobProvider trait specifies the functionality of a data source that can provide blobs.
#[async_trait]
pub trait BlobProvider {
    /// The error type for the [BlobProvider].
    type Error: Display + ToString + Into<PipelineErrorKind>;

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

    /// Fetches EigenDA data for a given commitment
    async fn retrieve_blob_with_commitment(
        &mut self,
        commitment: &[u8],
        blob_len: u32,
    ) -> Result<Vec<u8>, Self::Error>;

    /// Weather use mantle eigen-da indexer service
    fn da_indexer_enable(&mut self) -> bool;
}

/// Describes the functionality of a data source that can provide data availability information.
#[async_trait]
pub trait DataAvailabilityProvider {
    /// The item type of the data iterator.
    type Item: Send + Sync + Debug + Into<Bytes>;

    /// Returns the next data for the given [BlockInfo].
    /// Returns a `PipelineError::Eof` if there is no more data for the given block ref.
    async fn next(&mut self, block_ref: &BlockInfo) -> PipelineResult<Self::Item>;

    /// Clears the data source for the next block ref.
    fn clear(&mut self);
}
