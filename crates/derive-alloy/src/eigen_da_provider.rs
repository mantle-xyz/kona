//! Contains an online implementation of the `EigenDaProvider` trait.

use async_trait::async_trait;
use tokio::time::timeout;
use tonic::{Request};
use kona_derive::eigen_da::{ FramesAndDataRequest, IEigenDA};
use kona_derive::errors::{EigenDAProviderError};
use kona_derive::traits::EigenDAProvider;
use tonic::transport::{Channel};
use kona_derive::eigen_da::data_retrieval_client::DataRetrievalClient;
use tokio::time::Duration;


/// An online implementation of the [EigenDaProvider]
#[derive(Debug, Clone)]
pub struct OnlineEigenDaProvider<E: IEigenDA > {
    /// The EigenDA Proxy client.
    eigen_da_proxy_client: E,
    /// The Mantle da indexer socket url.
    pub mantle_da_indexer_socket: String,
    /// Whether you use mantle da indexer.
    pub mantle_da_indexer_enable: bool,
}

impl<E: IEigenDA > OnlineEigenDaProvider<E> {
    pub const fn new(
        eigen_da_proxy_client: E,
        mantle_da_indexer_socket: String,
        mantle_da_indexer_enable: bool,
    ) -> Self {
        Self{
            eigen_da_proxy_client,
            mantle_da_indexer_socket,
            mantle_da_indexer_enable,
        }
    }
}

#[async_trait]
impl<E> EigenDAProvider for OnlineEigenDaProvider<E>
where
    E: IEigenDA + Send + Sync,
{
    type Error = EigenDAProviderError;

    async fn retrieve_blob(&mut self, batch_header_hash: &[u8], blob_index: u32) -> Result<Vec<u8>, Self::Error> {
        self.eigen_da_proxy_client.retrieve_blob(batch_header_hash, blob_index).await
            .map_err(|e|EigenDAProviderError::String(e.to_string()))
    }

    async fn retrieve_blob_with_commitment(&mut self, commitment: &[u8]) -> Result<Vec<u8>, Self::Error> {
        self.eigen_da_proxy_client.retrieve_blob_with_commitment(commitment).await
            .map_err(|e|EigenDAProviderError::String(e.to_string()))
    }

    async fn retrieval_frames_from_da_indexer(&mut self, tx_hash: &str) -> Result<Vec<u8>, Self::Error> {

        let channel = Channel::from_shared(self.mantle_da_indexer_socket.clone())
            .map_err(|e|EigenDAProviderError::RetrieveFramesFromDaIndexer(e.to_string()))?
            .connect()
            .await
            .map_err(|e|EigenDAProviderError::RetrieveFramesFromDaIndexer(e.to_string()))?;


        let mut client = DataRetrievalClient::new(channel);

        let ctx = timeout(Duration::from_secs(60), async {
            let request = Request::new(FramesAndDataRequest {
                data_confirm_hash: tx_hash.to_string(),
            });
            client.retrieve_frames_and_data(request).await
        }).await;

        let response = match ctx {
            Ok(Ok(reply)) => reply.into_inner(),
            Ok(Err(err)) => return Err(EigenDAProviderError::RetrieveFramesFromDaIndexer(err.to_string())),
            Err(_) => return Err(EigenDAProviderError::TimeOut("Timeout while retrieving blob".into())),
        };

        Ok(response.data)

    }

    fn da_indexer_enable(&mut self) -> bool {
        self.mantle_da_indexer_enable
    }
}