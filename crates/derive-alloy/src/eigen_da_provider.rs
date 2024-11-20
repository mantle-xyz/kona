//! Contains an online implementation of the `EigenDaProvider` trait.

use async_trait::async_trait;
use kona_derive::eigen_da::{ IEigenDA};
use kona_derive::errors::{EigenDAProviderError};
use kona_derive::traits::EigenDAProvider;


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

    pub async fn get_blob(&self,commitment: &[u8]) -> Result<Vec<u8>, EigenDAProviderError> {
        self.eigen_da_proxy_client.retrieve_blob_with_commitment(commitment).await
            .map_err(|e|EigenDAProviderError::String(e.to_string()))
    }
}

#[async_trait]
impl<E> EigenDAProvider for OnlineEigenDaProvider<E>
where
    E: IEigenDA + Send + Sync,
{
    type Error = EigenDAProviderError;

    async fn retrieve_blob_with_commitment(&mut self, commitment: &[u8]) -> Result<Vec<u8>, Self::Error> {
        self.eigen_da_proxy_client.retrieve_blob_with_commitment(commitment).await
            .map_err(|e|EigenDAProviderError::String(e.to_string()))
    }

    fn da_indexer_enable(&mut self) -> bool {
        self.mantle_da_indexer_enable
    }
}