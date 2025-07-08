use crate::HintType;
use crate::errors::OracleProviderError;
use alloc::boxed::Box;
use alloc::string::ToString;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use alloy_primitives::Bytes;
use async_trait::async_trait;
use kona_derive::traits::EigenDAProvider;
use kona_eigenda::{
    EigenDABlobData, decode_blob_info_from_commitment, create_blob_key_template,
    update_blob_key_with_index, calculate_blob_key_hash, calculate_blob_size_bytes,
    FIELD_ELEMENT_SIZE
};
use kona_preimage::errors::PreimageOracleError;
use kona_preimage::{CommsClient, PreimageKey, PreimageKeyType};
use tracing::debug;

/// An oracle-backed eigenDA provider.
#[derive(Debug, Clone)]
pub struct OracleEigenDaProvider<T: CommsClient> {
    /// The preimage oracle client.
    pub oracle: Arc<T>,
}

impl<T: CommsClient> OracleEigenDaProvider<T> {
    /// Constructs a new `OracleBlobProvider`.
    pub const fn new(oracle: Arc<T>) -> Self {
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
    async fn get_blob(
        &self,
        commitment: &[u8],
    ) -> Result<Vec<u8>, OracleProviderError> {
        HintType::EigenDABlob.with_data(&[commitment]).send(self.oracle.as_ref()).await?;

        // Decode blob info from commitment (skip metadata)
        let cert_blob_info = decode_blob_info_from_commitment(commitment)
            .map_err(|_| OracleProviderError::Preimage(PreimageOracleError::Other(
                "Commitment does not contain required header".into(),
            )))?;
        debug!("Decoded cert blob info: {:?}", cert_blob_info);

        // Calculate blob size (data_length measures in field elements, multiply to get bytes)
        let field_element_count = cert_blob_info.blob_header.data_length as u64;
        let blob_size_bytes = calculate_blob_size_bytes(field_element_count);
        debug!("Field element count: {}, blob size: {} bytes", field_element_count, blob_size_bytes);

        // Initialize blob buffer
        let mut blob = vec![0u8; blob_size_bytes];

        // Prepare blob key template for field element retrieval
        let mut blob_key = create_blob_key_template(&cert_blob_info);

        // Retrieve each field element from the oracle
        for field_index in 0..field_element_count {
            // Update blob key with current field index
            update_blob_key_with_index(&mut blob_key, field_index);

            // Retrieve field element from oracle
            let mut field_element = [0u8; FIELD_ELEMENT_SIZE];
            let key_hash = calculate_blob_key_hash(&blob_key);
            let preimage_key = PreimageKey::new(key_hash, PreimageKeyType::GlobalGeneric);
            
            self.oracle
                .get_exact(preimage_key, &mut field_element)
                .await
                .map_err(OracleProviderError::Preimage)?;

            // Validate field element is not empty (indicates EigenDA invariant breach)
            if field_element.is_empty() {
                return Err(OracleProviderError::Preimage(PreimageOracleError::Other(
                    "Field element is empty, breached EigenDA invariant".into(),
                )));
            }

            // Copy field element to blob at correct position
            let blob_start = field_index as usize * FIELD_ELEMENT_SIZE;
            let blob_end = blob_start + FIELD_ELEMENT_SIZE;
            blob[blob_start..blob_end].copy_from_slice(field_element.as_ref());
        }

        debug!(target: "client_oracle", "Retrieved blob from EigenDA with commitment {commitment:?} from oracle");
        
        // Decode the blob data from EigenDA format
        let eigenda_blob_data = EigenDABlobData::new(Bytes::copy_from_slice(&blob));
        let decoded_blob = eigenda_blob_data.decode()
            .map_err(|err| OracleProviderError::Preimage(PreimageOracleError::Other(err.to_string())))?;

        Ok(decoded_blob.to_vec())
    }
}

#[async_trait]
impl<T: CommsClient + Sync + Send> EigenDAProvider for OracleEigenDaProvider<T> {
    type Error = OracleProviderError;

    async fn retrieve_blob_with_commitment(
        &mut self,
        commitment: &[u8],
    ) -> Result<Vec<u8>, Self::Error> {
        debug!("Starting to retrieve blob from EigenDA with commitment: {:?}", commitment);
        let blob_data = self.get_blob(commitment).await?;
        Ok(blob_data)
    }
}
