use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::vec;
use alloy_primitives::keccak256;
use async_trait::async_trait;
// use tokio::io::AsyncReadExt;
use kona_derive::traits::EigenDAProvider;
use kona_preimage::{CommsClient, PreimageKey, PreimageKeyType};
use kona_preimage::PreimageKeyType::Precompile;
use crate::errors::OracleProviderError;
use crate::HintType;

#[derive(Debug,Clone)]
pub struct OracleEigenDaProvider<T: CommsClient> {
    /// The preimage oracle client.
    pub oracle: Arc<T>,
}


impl<T: CommsClient> OracleEigenDaProvider<T> {
    /// Constructs a new `OracleBlobProvider`.
    pub fn new(oracle: Arc<T>) -> Self {
        Self { oracle }
    }

    /// Retrieves a blob from the oracle.
    ///
    /// ## Takes
    /// - `commitment`: The blob commitment.
    ///
    /// ## Returns
    /// - `Ok(blob)`: The blob.
    /// - `Err(e)`: The blob could not be retrieved.
    async fn get_blob(&self, commitment: &[u8], blob_len: u32) -> Result<Vec<u8>, OracleProviderError> {
        self.oracle.write(&HintType::EigenDa.encode_with(&[commitment.as_ref()]))
            .await
            .map_err(OracleProviderError::Preimage)?;
        let mut out_data = vec![0u8; blob_len as usize];
        self.oracle.get_exact(PreimageKey::new(*keccak256(commitment),PreimageKeyType::GlobalGeneric), &mut out_data)
            .await
            .map_err(OracleProviderError::Preimage)?;
        tracing::info!(target: "client_oracle", "Retrieved blob from eigen da with commitment {commitment:?} from the oracle.");
        Ok(out_data)
    }

}

#[async_trait]
impl<T: CommsClient + Sync + Send> EigenDAProvider for OracleEigenDaProvider<T> {
    type Error = OracleProviderError;

    async fn retrieve_blob_with_commitment(&mut self, commitment: &[u8], blob_len: u32) -> Result<Vec<u8>, Self::Error> {
        trace!("Start to get blobs from eigen da with commitment {:?}", commitment);
        let out_data:Vec<u8> = self.get_blob(commitment, blob_len).await?;
        Ok(out_data)
    }

    fn da_indexer_enable(&mut self) -> bool {
        false
    }
}
