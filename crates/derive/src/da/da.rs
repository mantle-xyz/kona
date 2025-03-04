use alloc::boxed::Box;
use alloc::vec::Vec;
use async_trait::async_trait;
use core::fmt::Display;

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
