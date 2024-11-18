use alloc::{format, vec};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::time::Duration;
use anyhow::anyhow;
use async_trait::async_trait;
use prost::Message;
use reqwest::Client;
use rlp::{decode, RlpStream};
use tokio::time::error::Elapsed;
use tokio::time::timeout;
use tonic::codegen::http::StatusCode;
use tonic::Request;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use crate::eigen_da::codec;
use crate::eigen_da::config::EigenDaConfig;
use crate::eigen_da::da::IEigenDA;
use crate::eigen_da::grpc::{BlobInfo, BlobStatusReply, BlobStatusRequest, DisperserClient, RetrieveBlobRequest};
use crate::errors::EigenDAProxyError;
use alloc::boxed::Box;
use bytes::Bytes;

pub const CERT_V0: u8 = 0;
pub const EIGEN_DA_COMMITMENT_TYPE: u8 = 0;
pub const GENERIC_COMMITMENT_TYPE: u8 = 1;

pub const BYTES_PER_SYMBOL:usize = 32;


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
            disperse_client: Client::builder().timeout(cfg.disperse_blob_timeout).build().expect("disperse client builder failed"),
            retrieve_client: Client::builder().timeout(cfg.retrieve_blob_timeout).build().expect("retrieve client builder failed"),
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

        if op_type != GENERIC_COMMITMENT_TYPE || da_provider != EIGEN_DA_COMMITMENT_TYPE || cert_version != CERT_V0 {
            anyhow::bail!("invalid commitment type");
        }

        let data = &commitment[3..];
        let blob_info: BlobInfo = decode(data).map_err(|e| anyhow!("unable to decode commitment: {}", e))?;
        Ok(blob_info)
    }

    pub fn encode_commitment(blob_info: BlobInfo) -> Result<Vec<u8>, anyhow::Error> {
        let mut blob_info_bytes = Vec::new();
        if let Err(err)  = blob_info.encode(&mut blob_info_bytes) {
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

    async fn retrieve_blob(&self, batch_header_hash: &[u8], blob_index: u32) -> Result<Vec<u8>, Self::Error> {

        //config TLS
        let tls_config = ClientTlsConfig::new().domain_name(format!("{}.tls", self.proxy_url));
        let channel = Channel::from_shared(self.disperser_url.clone()).map_err(|e|EigenDAProxyError::RetrieveBlob(e.to_string()))?
            .tls_config(tls_config).map_err(|e|EigenDAProxyError::RetrieveBlob(e.to_string()))?
            .timeout(self.retrieve_blob_timeout)
            .connect()
            .await.map_err(|e|EigenDAProxyError::RetrieveBlob(e.to_string()))?;
        let mut da_client = DisperserClient::new(channel);
        let ctx = timeout(self.retrieve_blob_timeout, async {
            let request = Request::new(RetrieveBlobRequest {
                batch_header_hash: Vec::from(batch_header_hash.clone()),
                blob_index,
            });
            da_client.retrieve_blob(request).await
        }).await;

        let response = match ctx {
            Ok(Ok(reply)) => reply.into_inner(),
            Ok(Err(err)) => return Err(EigenDAProxyError::RetrieveBlob(err.to_string())),
            Err(_) => return Err(EigenDAProxyError::RetrieveBlob("Timeout while retrieving blob".into())),
        };
        let decode_data = codec::remove_empty_byte_from_padded_bytes(&response.data);
        Ok(decode_data)
    }

    async fn retrieve_blob_with_commitment(&self, commitment: &[u8]) -> Result<Vec<u8>, Self::Error> {
        let blob_info = Self::decode_commitment(commitment).map_err(|e|EigenDAProxyError::RetrieveBlobWithCommitment(e.to_string()))?;
        let request_url = format!("{}/get/0x{}", self.proxy_url, hex::encode(&commitment));
        let req = self.retrieve_client.get(&request_url);
        let response =  timeout(self.retrieve_blob_timeout, req.send())
            .await.map_err(|e|EigenDAProxyError::RetrieveBlobWithCommitment(e.to_string()))?
            .map_err(|e|EigenDAProxyError::RetrieveBlobWithCommitment(e.to_string()))?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(EigenDAProxyError::RetrieveBlobWithCommitment("Blob not found".into()));
        } else if response.status() != StatusCode::OK {
            return Err(EigenDAProxyError::RetrieveBlobWithCommitment(format!(
                "Failed to get preimage, status: {}",
                response.status()
            )));
        }


        let body = response.bytes().await.map_err(|e| EigenDAProxyError::RetrieveBlobWithCommitment(e.to_string()))?;
        Ok(body.to_vec())
    }

    async fn disperse_blob(&self, tx_data: &[u8]) -> Result<BlobInfo, Self::Error> {
        if tx_data.is_empty() {
            return  Err(EigenDAProxyError::InvalidInput);
        }
        let url = format!("{}/put/", self.proxy_url);
        let request = self
            .disperse_client
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .body(Bytes::from(tx_data.to_vec()));

        let response = timeout(self.retrieve_blob_timeout, request.send()).await
            .map_err(|e: Elapsed| EigenDAProxyError::TimeOut(e.to_string()))?
            .map_err(|e|EigenDAProxyError::DisperseBlob(e.to_string()))?;

        if response.status() != StatusCode::OK {
            return Err(EigenDAProxyError::DisperseBlob(
                format!("Failed to store data: {}", response.status()),
            ));
        }


        let response_body = response.bytes().await.map_err(|e| EigenDAProxyError::RetrieveBlob(e.to_string()))?;

        let comm = Self::decode_commitment(&response_body).map_err(|e| EigenDAProxyError::DisperseBlob(e.to_string()))?;

        Ok(comm)
    }

    async fn get_blob_status(&self, request_id: &[u8]) -> Result<BlobStatusReply, Self::Error> {
        //config TLS
        let tls_config = ClientTlsConfig::new().domain_name(format!("{}.tls", self.proxy_url));
        let channel = Channel::from_shared(self.disperser_url.clone()).map_err(|e|EigenDAProxyError::GetBlobStatus(e.to_string()))?
            .tls_config(tls_config).map_err(|e|EigenDAProxyError::GetBlobStatus(e.to_string()))?
            .timeout(self.retrieve_blob_timeout)
            .connect()
            .await.map_err(|e|EigenDAProxyError::GetBlobStatus(e.to_string()))?;
        let mut da_client = DisperserClient::new(channel);
        let request = Request::new(BlobStatusRequest {
            request_id: request_id.to_vec()
        });
        let response = timeout(self.retrieve_blob_timeout, da_client.get_blob_status(request))
            .await.map_err(|e| EigenDAProxyError::TimeOut(e.to_string()))?
            .map_err(|e|EigenDAProxyError::GetBlobStatus(e.to_string()))?;
        Ok(response.into_inner())
    }


}

