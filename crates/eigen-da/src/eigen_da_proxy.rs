use crate::config::EigenDaConfig;
use crate::grpc::BlobInfo;
use anyhow::anyhow;
use async_trait::async_trait;
use core::time::Duration;
use kona_derive::da::IEigenDA;
use kona_derive::errors::EigenDAProxyError;
use prost::Message;
use reqwest::{Client, StatusCode};
use rlp::{decode, RlpStream};
use tokio::time::timeout;
use Box;
use Vec;
use {format, vec};
use {String, ToString};

pub const CERT_V0: u8 = 0;
pub const EIGEN_DA_COMMITMENT_TYPE: u8 = 0;
pub const GENERIC_COMMITMENT_TYPE: u8 = 1;

pub const BYTES_PER_SYMBOL: usize = 32;

/// An implementation of the [IEigenDA] trait.
#[derive(Debug, Clone)]
pub struct EigenDaProxy {
    /// The url of EigenDA proxy service.
    pub proxy_url: String,
    /// The url of EigenDA disperser service.
    pub disperser_url: String,
    /// The http client of EigenDA disperser service.
    pub disperse_client: Client,
    /// The http client of EigenDA retrieve service.
    pub retrieve_client: Client,
    /// The timeout for request form retrieve service.
    pub retrieve_blob_timeout: Duration,
}

impl EigenDaProxy {
    /// create a new EigenDA Proxy client.
    pub fn new(cfg: EigenDaConfig) -> Self {
        Self {
            proxy_url: cfg.proxy_url,
            disperser_url: cfg.disperse_url,
            disperse_client: Client::builder()
                .timeout(cfg.disperse_blob_timeout)
                .build()
                .expect("disperse client builder failed"),
            retrieve_client: Client::builder()
                .timeout(cfg.retrieve_blob_timeout)
                .build()
                .expect("retrieve client builder failed"),
            retrieve_blob_timeout: cfg.retrieve_blob_timeout,
        }
    }

    /// decode commitment which get from EigenDA
    pub fn decode_commitment(commitment: &[u8]) -> Result<BlobInfo, anyhow::Error> {
        if commitment.len() < 3 {
            anyhow::bail!("commitment is too short");
        }

        let op_type = commitment[0];
        let da_provider = commitment[1];
        let cert_version = commitment[2];

        if op_type != GENERIC_COMMITMENT_TYPE
            || da_provider != EIGEN_DA_COMMITMENT_TYPE
            || cert_version != CERT_V0
        {
            anyhow::bail!("invalid commitment type");
        }

        let data = &commitment[3..];
        let blob_info: BlobInfo =
            decode(data).map_err(|e| anyhow!("unable to decode commitment: {}", e))?;
        Ok(blob_info)
    }

    pub fn encode_commitment(blob_info: BlobInfo) -> Result<Vec<u8>, anyhow::Error> {
        let mut blob_info_bytes = Vec::new();
        if let Err(err) = blob_info.encode(&mut blob_info_bytes) {
            anyhow::bail!(err);
        }
        let mut stream = RlpStream::new();
        stream.append(&blob_info_bytes);
        let rlp_encoded_bytes = stream.out();
        let mut result = vec![GENERIC_COMMITMENT_TYPE, EIGEN_DA_COMMITMENT_TYPE, CERT_V0];
        result.extend(rlp_encoded_bytes);
        Ok(result)
    }
}

#[async_trait]
impl IEigenDA for EigenDaProxy {
    type Error = EigenDAProxyError;

    async fn retrieve_blob_with_commitment(
        &self,
        commitment: &[u8],
    ) -> Result<Vec<u8>, Self::Error> {
        let blob_info = Self::decode_commitment(commitment)
            .map_err(|e| EigenDAProxyError::RetrieveBlobWithCommitment(e.to_string()))?;
        let request_url = format!("{}/get/0x{}", self.proxy_url, hex::encode(&commitment));
        let req = self.retrieve_client.get(&request_url);
        let response = timeout(self.retrieve_blob_timeout, req.send())
            .await
            .map_err(|e| EigenDAProxyError::RetrieveBlobWithCommitment(e.to_string()))?
            .map_err(|e| EigenDAProxyError::RetrieveBlobWithCommitment(e.to_string()))?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(EigenDAProxyError::RetrieveBlobWithCommitment("Blob not found".into()));
        } else if response.status() != StatusCode::OK {
            return Err(EigenDAProxyError::RetrieveBlobWithCommitment(format!(
                "Failed to get preimage, status: {}",
                response.status()
            )));
        }

        let body = response
            .bytes()
            .await
            .map_err(|e| EigenDAProxyError::RetrieveBlobWithCommitment(e.to_string()))?;
        Ok(body.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::hex;
    #[test]
    fn test_decode_blob_info() {
        let commitment = hex!("010000f901d8f852f842a00dbbd22149b419a9a751c25065b58745f4216dc3ae4e9ad583306c395387b6a3a02673dfa25dd3095246eeffb639d3e11108a1ba75dd29b86c3a4200ed00210e4e820200cac480213701c401213710f90181830148ae81a5f873eba0c42bcd27bcd22ba55c4189a25d362343838cb75f57979baa0686ec5381a944c3820001826362832a79cba07263089b84cbb2963e4f50a930243c081ab14b01c0c92d57c3029590bd9dfc9200832a7a20a05419bc29ac025512311c14f23d9613e408448e47bb31f71614e1f82b6c63966cb9010074b13a3acaba35d3749063c19806c9a2f2004b318d55edd6cb5129d958807ea7ac09584a2c6ea029ed34c72f849862e4189928e90931e07093209016f5fc70a6c4a8c3237c25c4f236bb25c105fd7dbd6e4a00153c69c0757d8cbf02f966167ccae243412c20de1c3a38a50818dc7f9f3e02dcb3bc4e54800f2224b8c1eaa9955e41792fa0e401f2814ee209331126149c630c34e1b8e2f804955582022676e232d24d7784b496fc997d98db2849b1bfa8443b362723fc603da8de11704a1ef50414e11234496cfac67aebdd2faa24840ffe7f04506652b8a11a534b024a40bc7e99fee042336f425eb16e40e4267593415860204c9069723dbaca8cf2e596dc820001");
        let blob_info = EigenDaProxy::decode_commitment(&commitment).unwrap();
        let blob_header = blob_info.blob_header.unwrap();
        assert_eq!(blob_header.data_length, 512);
        let blob_proof = blob_info.blob_verification_proof.unwrap();
        assert_eq!(blob_proof.blob_index, 165);
        assert_eq!(blob_proof.batch_id, 84142);
    }
}
