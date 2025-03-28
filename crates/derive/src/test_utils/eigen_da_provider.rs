use crate::errors::EigenDAProviderError;
use crate::traits::EigenDAProvider;
use alloc::boxed::Box;
use alloc::string::ToString;
use alloc::vec::Vec;
use async_trait::async_trait;

/// A mock blob provider for testing.
#[derive(Debug, Clone, Default)]
pub struct TestEigenDaProvider {
    /// blob data.
    pub blob: Vec<u8>,
    /// whether the blob provider should return an error.
    pub should_error: bool,
}

#[async_trait]
impl EigenDAProvider for TestEigenDaProvider {
    type Error = EigenDAProviderError;

    async fn retrieve_blob_with_commitment(
        &mut self,
        commitment: &[u8],
        blob_len: u32,
    ) -> Result<Vec<u8>, Self::Error> {
        if self.should_error {
            return Err(EigenDAProviderError::Blob("error".to_string()));
        }
        Ok(self.blob.clone())
    }

    fn da_indexer_enable(&mut self) -> bool {
        false
    }
}
