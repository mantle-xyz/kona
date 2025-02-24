use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::string::ToString;
use alloc::vec;
use alloy_primitives::{keccak256, Bytes};
use alloy_rlp::Decodable;
use async_trait::async_trait;
use eigen_da::{BlobInfo, EigenDABlobData, BYTES_PER_FIELD_ELEMENT};
// use tokio::io::AsyncReadExt;
use kona_derive::traits::EigenDAProvider;
use kona_preimage::{CommsClient, PreimageKey, PreimageKeyType};
use kona_preimage::errors::PreimageOracleError;
use kona_preimage::PreimageKeyType::Precompile;
use crate::errors::OracleProviderError;
use crate::HintType;

#[derive(Debug, Clone)]
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
        HintType::EigenDa.with_data(&[commitment.as_ref()]).send(self.oracle.as_ref()).await?;


        // the fourth because 0x010000 in the beginning is metadata
        // cert should at least contain 32 bytes for header + 3 bytes for commitment type metadata
        if commitment.len() <= 32 + 3 {
            return Err(OracleProviderError::Preimage(PreimageOracleError::Other(
                "does not contain header".into(),
            )));
        }

        // the first four bytes are metadata, like cert version, OP generic commitement
        // see https://github.com/Layr-Labs/eigenda-proxy/blob/main/commitments/mode.go#L39
        // the first byte my guess is the OP
        let cert_blob_info = BlobInfo::decode(&mut &commitment[3..]).unwrap();
        tracing::info!("cert_blob_info {:?}", cert_blob_info);

        // data_length measurs in field element, multiply to get num bytes
        // let mut blob: Vec<u8> =
        //     vec![0; cert_blob_info.blob_header.data_length as usize * BYTES_PER_FIELD_ELEMENT];

        // 96 because our g1 commitment has 64 bytes in v1
        // why 96, the original 4844 has bytes length of 80 (it has 48 bytes for commitment)
        // even then, it is not that the entire 80 bytes are used. Some bytes are empty
        // for solidity optimization, I remember.
        //
        // TODO: investigate later to decide a right size
        let mut blob_key = [0u8; 65];

        // In eigenDA terminology, length describes the number of field element, size describes
        // number of bytes.
        // let data_length = cert_blob_info.blob_header.data_length as u64;

        // tracing::info!("cert_blob_info.blob_header.data_length {:?}", data_length);

        // the common key
        blob_key[..32].copy_from_slice(&cert_blob_info.blob_header.commitment.x);
        blob_key[32..64].copy_from_slice(&cert_blob_info.blob_header.commitment.y);

        blob_key[64..].copy_from_slice(0i8.to_be_bytes().as_ref());
        let mut out_data = vec![0u8; blob_len as usize];
        self.oracle
            .get_exact(
                PreimageKey::new(*keccak256(blob_key), PreimageKeyType::GlobalGeneric),
                &mut out_data,
            )
            .await
            .map_err(OracleProviderError::Preimage)?;
        // + 1 for the proof
        // for i in 0..data_length {
        //     blob_key[88..].copy_from_slice(i.to_be_bytes().as_ref());
        //
        //     let mut field_element = [0u8; 32];
        //     self.oracle
        //         .get_exact(
        //             PreimageKey::new(*keccak256(blob_key), PreimageKeyType::GlobalGeneric),
        //             &mut field_element,
        //         )
        //         .await
        //         .map_err(OracleProviderError::Preimage)?;
        //
        //     // if field element is 0, it means the host has identified that the data
        //     // has breached eigenda invariant, i.e cert is valid
        //     if field_element.is_empty() {
        //         return Err(OracleProviderError::Preimage(PreimageOracleError::Other(
        //             "field elememnt is empty, breached eigenda invariant".into(),
        //         )));
        //     }
        //
        //     blob[(i as usize) << 5..(i as usize + 1) << 5].copy_from_slice(field_element.as_ref());
        // }

        // tracing::info!(target: "client_oracle", "Retrieved blob from eigen da with commitment {commitment:?} from the oracle.");
        // let eigenda_blob_data = EigenDABlobData::new(Bytes::copy_from_slice(&blob));
        // let blobs = eigenda_blob_data.decode();
        //
        // blobs
        //     .map_err(|err| {
        //     OracleProviderError::Preimage(PreimageOracleError::Other(err.to_string()))
        // })
        //     .map(|blob_data| blob_data.to_vec())
        Ok(out_data)
    }
}

#[async_trait]
impl<T: CommsClient + Sync + Send> EigenDAProvider for OracleEigenDaProvider<T> {
    type Error = OracleProviderError;

    async fn retrieve_blob_with_commitment(
        &mut self,
        commitment: &[u8],
        blob_len: u32,
    ) -> Result<Vec<u8>, Self::Error> {
        trace!("Start to get blobs from eigen da with commitment {:?}", commitment);
        let out_data: Vec<u8> = self.get_blob(commitment, blob_len).await?;
        Ok(out_data)
    }

    fn da_indexer_enable(&mut self) -> bool {
        false
    }
}
