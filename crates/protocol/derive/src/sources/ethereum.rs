//! Contains the [EthereumDataSource], which is a concrete implementation of the
//! [DataAvailabilityProvider] trait for the Ethereum protocol.

use crate::{
    sources::{BlobSource, CalldataSource, EigenDaSource},
    traits::{BlobProvider, ChainProvider, DataAvailabilityProvider, EigenDAProvider},
    types::PipelineResult,
};
use alloc::{boxed::Box, fmt::Debug};
use alloy_primitives::{Address, Bytes};
use async_trait::async_trait;
use kona_genesis::RollupConfig;
use kona_protocol::BlockInfo;

/// A factory for creating an Ethereum data source provider.
#[derive(Debug, Clone)]
pub struct EthereumDataSource<C, B, E>
where
    C: ChainProvider + Send + Clone,
    B: BlobProvider + Send + Clone,
    E: EigenDAProvider + Send + Debug + Clone,
{
    /// The ecotone timestamp.
    pub ecotone_timestamp: Option<u64>,
    /// The blob source.
    pub blob_source: BlobSource<C, B>,
    /// The calldata source.
    pub calldata_source: CalldataSource<C>,
    /// The eigen da source.
    pub eigen_da_source: EigenDaSource<C, B, E>,
}

impl<C, B, E> EthereumDataSource<C, B, E>
where
    C: ChainProvider + Send + Clone + Debug,
    B: BlobProvider + Send + Clone + Debug,
    E: EigenDAProvider + Send + Debug + Clone,
{
    // [TODO]: add cfg.matnle_da_swtich !!
    /// Instantiates a new [EthereumDataSource].
    pub const fn new(
        blob_source: BlobSource<C, B>,
        calldata_source: CalldataSource<C>,
        eigen_da_source: EigenDaSource<C, B, E>,
        cfg: &RollupConfig,
    ) -> Self {
        Self {
            ecotone_timestamp: cfg.hardforks.ecotone_time,
            blob_source,
            calldata_source,
            eigen_da_source,
        }
    }

    /// Instantiates a new [EthereumDataSource] from parts.
    pub fn new_from_parts(provider: C, blobs: B, eigen_da_provider: E, cfg: &RollupConfig) -> Self {
        Self {
            ecotone_timestamp: cfg.hardforks.ecotone_time,
            blob_source: BlobSource::new(provider.clone(), blobs.clone(), cfg.batch_inbox_address),
            calldata_source: CalldataSource::new(provider.clone(), cfg.batch_inbox_address),
            eigen_da_source: EigenDaSource::new(
                provider,
                blobs,
                eigen_da_provider,
                cfg.batch_inbox_address,
            ),
        }
    }
}

#[async_trait]
impl<C, B, E> DataAvailabilityProvider for EthereumDataSource<C, B, E>
where
    C: ChainProvider + Send + Sync + Clone + Debug,
    B: BlobProvider + Send + Sync + Clone + Debug,
    E: EigenDAProvider + Send + Sync + Debug + Clone,
{
    type Item = Bytes;

    async fn next(
        &mut self,
        block_ref: &BlockInfo,
        batcher_address: Address,
    ) -> PipelineResult<Self::Item> {
        // let ecotone_enabled =
        //     self.ecotone_timestamp.map(|e| block_ref.timestamp >= e).unwrap_or(false);
        // if ecotone_enabled {
        //     self.blob_source.next(block_ref, batcher_address).await
        // } else {
        //     self.calldata_source.next(block_ref, batcher_address).await
        // }
        self.eigen_da_source.next(block_ref, batcher_address).await
    }

    fn clear(&mut self) {
        self.blob_source.clear();
        self.calldata_source.clear();
        self.eigen_da_source.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        test_utils::{TestBlobProvider, TestChainProvider, TestEigenDaProvider},
    };
    use alloc::vec;
    use alloy_primitives::Address;
    use kona_genesis::RollupConfig;

    #[tokio::test]
    async fn test_clear_ethereum_data_source() {
        let chain = TestChainProvider::default();
        let blob_fetcher = TestBlobProvider::default();
        let cfg = RollupConfig::default();
        let mut calldata = CalldataSource::new(chain.clone(), Address::ZERO);
        calldata.calldata.insert(0, Default::default());
        calldata.open = true;
        let mut blob = BlobSource::new(chain.clone(), blob_fetcher.clone(), Address::ZERO);
        blob.data = vec![Default::default()];
        blob.open = true;
        let eigen_da_provider = TestEigenDaProvider::new();
        let mut eigen = EigenDaSource::new(
            chain,
            blob_fetcher,
            eigen_da_provider,
            Address::ZERO,
        );
        eigen.data = vec![Default::default()];
        eigen.open = true;
        let mut data_source = EthereumDataSource::new(blob, calldata, eigen, &cfg);

        data_source.clear();
        assert!(data_source.blob_source.data.is_empty());
        assert!(!data_source.blob_source.open);
        assert!(data_source.calldata_source.calldata.is_empty());
        assert!(!data_source.calldata_source.open);
        assert!(data_source.eigen_da_source.data.is_empty());
        assert!(!data_source.eigen_da_source.open);
    }
}
