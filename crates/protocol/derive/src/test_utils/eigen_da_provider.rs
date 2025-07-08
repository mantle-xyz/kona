use crate::errors::EigenDAProviderError;
use crate::traits::EigenDAProvider;
use alloc::boxed::Box;
use alloc::string::ToString;
use alloc::vec;
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

impl TestEigenDaProvider {
    /// Create a new [TestEigenDaProvider] with test data.
    pub fn new() -> Self {
        Self { blob: vec![0u8; 12000], should_error: false }
    }
}

#[async_trait]
impl EigenDAProvider for TestEigenDaProvider {
    type Error = EigenDAProviderError;

    async fn retrieve_blob_with_commitment(
        &mut self,
        _commitment: &[u8],
    ) -> Result<Vec<u8>, Self::Error> {
        if self.should_error {
            return Err(EigenDAProviderError::Backend("error".to_string()));
        }
        Ok(self.blob.clone())
    }
}
