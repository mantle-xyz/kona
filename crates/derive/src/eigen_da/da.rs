use alloc::vec::Vec;
use core::fmt::Display;
use async_trait::async_trait;

use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use bytes::Bytes;
use crate::eigen_da::grpc::{BlobInfo, BlobStatusReply};
use crate::traits::BlobProvider;

#[async_trait]
pub trait IEigenDA {

    /// The error type for the [IEigenDA].
    type Error: Display;

    /// get blob from EigenDA with batch header hash and blob index
    async fn retrieve_blob(
        &self,
        batch_header_hash: &[u8],
        blob_index: u32,
    ) -> Result<Vec<u8>, Self::Error>;

    /// get blob from EigenDA with commitment
    async fn retrieve_blob_with_commitment(
        &self,
        commitment: &[u8],
    ) -> Result<Vec<u8>, Self::Error>;

    /// get BlobInfo from EigenDA with tx_data bytes from EigenDA
    async fn disperse_blob(
        &self,
        tx_data: &[u8],
    ) -> Result<BlobInfo, Self::Error>;

    /// get blob status from EigenDA with request id from EigenDA
    async fn get_blob_status(
        &self,
        request_id: &[u8],
    ) -> Result<BlobStatusReply, Self::Error>;
}