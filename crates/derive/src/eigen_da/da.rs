use alloc::vec::Vec;
use core::fmt::Display;
use async_trait::async_trait;
use crate::eigen_da::grpc::{BlobInfo, BlobStatusReply};
use crate::traits::BlobProvider;
use alloc::boxed::Box;

#[async_trait]
pub trait IEigenDA {

    /// The error type for the [IEigenDA].
    type Error: Display;

    /// get blob from EigenDA with commitment
    async fn retrieve_blob_with_commitment(
        &self,
        commitment: &[u8],
    ) -> Result<Vec<u8>, Self::Error>;

}