//! Contains the [EthereumDataSource], which is a concrete implementation of the
//! [DataAvailabilityProvider] trait for the Ethereum protocol.

use crate::sources::eigen_da::EigenDaSource;
use crate::{
    sources::{BlobSource, CalldataSource},
    traits::{BlobProvider, ChainProvider, DataAvailabilityProvider, EigenDAProvider},
    types::PipelineResult,
};
use alloc::{boxed::Box, fmt::Debug};
use alloy_primitives::{Address, Bytes};
use async_trait::async_trait;
use op_alloy_genesis::RollupConfig;
use op_alloy_protocol::BlockInfo;

/// A factory for creating an Ethereum data source provider.
#[derive(Debug, Clone)]
pub struct EthereumDataSource<C, B, E>
where
    C: ChainProvider + Send + Clone,
    B: BlobProvider + Send + Clone,
    E: EigenDAProvider + Send + Clone,
{
    /// The calldata source.
    pub calldata_source: CalldataSource<C>,
    /// The eigen da source.
    pub eigen_da_source: EigenDaSource<C, B, E>,
    /// Mantle da switch
    pub mantle_da_switch: bool,
}

impl<C, B, E> EthereumDataSource<C, B, E>
where
    C: ChainProvider + Send + Clone + Debug,
    B: BlobProvider + Send + Clone + Debug,
    E: EigenDAProvider + Send + Clone + Debug,
{
    /// Instantiates a new [EthereumDataSource].
    pub const fn new(
        calldata_source: CalldataSource<C>,
        eigen_da_source: EigenDaSource<C, B, E>,
        cfg: &RollupConfig,
    ) -> Self {
        Self { calldata_source, eigen_da_source, mantle_da_switch: cfg.mantle_da_switch }
    }

    /// Creates a new factory.
    pub fn new_from_parts(provider: C, blobs: B, eigen_da_provider: E, cfg: &RollupConfig) -> Self {
        let signer =
            cfg.genesis.system_config.as_ref().map(|sc| sc.batcher_address).unwrap_or_default();
        Self {
            calldata_source: CalldataSource::new(provider.clone(), cfg.batch_inbox_address, signer),
            eigen_da_source: EigenDaSource::new(
                provider.clone(),
                blobs.clone(),
                eigen_da_provider.clone(),
                cfg.batch_inbox_address,
                signer,
            ),
            mantle_da_switch: cfg.mantle_da_switch,
        }
    }
}

#[async_trait]
impl<C, B, E> DataAvailabilityProvider for EthereumDataSource<C, B, E>
where
    C: ChainProvider + Send + Sync + Clone + Debug,
    B: BlobProvider + Send + Sync + Clone + Debug,
    E: EigenDAProvider + Send + Sync + Clone + Debug,
{
    type Item = Bytes;

    async fn next(&mut self, block_ref: &BlockInfo) -> PipelineResult<Self::Item> {
        if self.mantle_da_switch {
            self.eigen_da_source.next(block_ref).await
        } else {
            self.calldata_source.next(block_ref).await
        }
    }

    fn clear(&mut self) {
        self.calldata_source.clear();
        self.eigen_da_source.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestEigenDaProvider;
    use crate::{
        sources::BlobData,
        test_utils::{TestBlobProvider, TestChainProvider},
    };
    use alloy_consensus::TxEnvelope;
    use alloy_eips::eip2718::Decodable2718;
    use alloy_primitives::{address, Address};
    use op_alloy_genesis::{RollupConfig, SystemConfig};
    use op_alloy_protocol::BlockInfo;

    fn default_test_blob_source() -> BlobSource<TestChainProvider, TestBlobProvider> {
        let chain_provider = TestChainProvider::default();
        let blob_fetcher = TestBlobProvider::default();
        let batcher_address = Address::default();
        let signer = Address::default();
        BlobSource::new(chain_provider, blob_fetcher, batcher_address, signer)
    }

    #[tokio::test]
    async fn test_clear_ethereum_data_source() {
        let chain = TestChainProvider::default();
        let blob = TestBlobProvider::default();
        let cfg = RollupConfig::default();
        let eigen_da = TestEigenDaProvider::default();
        let mut calldata = CalldataSource::new(chain.clone(), Address::ZERO, Address::ZERO);
        calldata.calldata.insert(0, Default::default());
        calldata.open = true;
        let mut eigen = EigenDaSource::new(chain, blob, eigen_da, Address::ZERO, Address::ZERO);
        eigen.data = vec![Default::default()];
        eigen.open = true;
        let mut data_source = EthereumDataSource::new(calldata, eigen, &cfg);

        data_source.clear();
        assert!(data_source.eigen_da_source.data.is_empty());
        assert!(!data_source.eigen_da_source.open);
        assert!(data_source.calldata_source.calldata.is_empty());
        assert!(!data_source.calldata_source.open);
    }

    #[tokio::test]
    async fn test_open_ethereum_calldata_source_pre_ecotone() {
        let mut chain = TestChainProvider::default();
        let blob = TestBlobProvider::default();
        let batcher_address = address!("6887246668a3b87F54DeB3b94Ba47a6f63F32985");
        let batch_inbox = address!("FF00000000000000000000000000000000000010");
        let block_ref = BlockInfo { number: 10, ..Default::default() };

        let mut cfg = RollupConfig::default();
        cfg.genesis.system_config = Some(SystemConfig { batcher_address, ..Default::default() });
        cfg.batch_inbox_address = batch_inbox;

        // load a test batcher transaction
        let raw_batcher_tx = include_bytes!("../../testdata/raw_batcher_tx.hex");
        let tx = TxEnvelope::decode_2718(&mut raw_batcher_tx.as_ref()).unwrap();
        chain.insert_block_with_transactions(10, block_ref, alloc::vec![tx]);

        // Should successfully retrieve a calldata batch from the block
        let mut data_source = EthereumDataSource::new_from_parts(chain, blob, &cfg);
        let calldata_batch = data_source.next(&block_ref).await.unwrap();
        assert_eq!(calldata_batch.len(), 119823);
    }
}
