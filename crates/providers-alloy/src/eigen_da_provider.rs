//! Contains an online implementation of the `EigenDaProvider` trait.

use async_trait::async_trait;
use kona_derive::da::{ IEigenDA};
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
            .map_err(|e|EigenDAProviderError::Status(e.to_string()))
    }
}

#[async_trait]
impl<E> EigenDAProvider for OnlineEigenDaProvider<E>
where
    E: IEigenDA + Send + Sync,
{
    type Error = EigenDAProviderError;

    async fn retrieve_blob_with_commitment(&mut self, commitment: &[u8], blob_len: u32) -> Result<Vec<u8>, Self::Error> {
        self.eigen_da_proxy_client.retrieve_blob_with_commitment(commitment).await
            .map_err(|e|EigenDAProviderError::Status(e.to_string()))
    }

    fn da_indexer_enable(&mut self) -> bool {
        self.mantle_da_indexer_enable
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::hex;
    use eigen_da::{EigenDaConfig, EigenDaProxy};
    use super::*;

    #[tokio::test]
    async fn test_get_blob() {
        let commitment = hex!("010000f901d8f852f842a00dbbd22149b419a9a751c25065b58745f4216dc3ae4e9ad583306c395387b6a3a02673dfa25dd3095246eeffb639d3e11108a1ba75dd29b86c3a4200ed00210e4e820200cac480213701c401213710f90181830148ae81a5f873eba0c42bcd27bcd22ba55c4189a25d362343838cb75f57979baa0686ec5381a944c3820001826362832a79cba07263089b84cbb2963e4f50a930243c081ab14b01c0c92d57c3029590bd9dfc9200832a7a20a05419bc29ac025512311c14f23d9613e408448e47bb31f71614e1f82b6c63966cb9010074b13a3acaba35d3749063c19806c9a2f2004b318d55edd6cb5129d958807ea7ac09584a2c6ea029ed34c72f849862e4189928e90931e07093209016f5fc70a6c4a8c3237c25c4f236bb25c105fd7dbd6e4a00153c69c0757d8cbf02f966167ccae243412c20de1c3a38a50818dc7f9f3e02dcb3bc4e54800f2224b8c1eaa9955e41792fa0e401f2814ee209331126149c630c34e1b8e2f804955582022676e232d24d7784b496fc997d98db2849b1bfa8443b362723fc603da8de11704a1ef50414e11234496cfac67aebdd2faa24840ffe7f04506652b8a11a534b024a40bc7e99fee042336f425eb16e40e4267593415860204c9069723dbaca8cf2e596dc820001");
        let eigen_config = EigenDaConfig::default();
        let eigen_da_proxy = EigenDaProxy::new(eigen_config);
        let eigen_da_provider = OnlineEigenDaProvider::new(eigen_da_proxy,"".to_string(),false);
        let out = eigen_da_provider.get_blob(&commitment).await.unwrap();
        assert_eq!(out.len(),11681)
    }


}
