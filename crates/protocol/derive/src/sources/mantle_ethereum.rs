//! Contains the [MantleEthereumDataSource], which is a concrete implementation of the
//! [DataAvailabilityProvider] trait for the Ethereum protocol with Mantle Arsia hardfork support.
//!
//! This data source handles blob decoding based on the Mantle Arsia hardfork:
//! - Before Mantle Arsia: uses MantleBlobSource (Mantle blob decoding)
//! - After Mantle Arsia: uses BlobSource (standard blob decoding)

use crate::{
    BlobProvider, BlobSource, CalldataSource, ChainProvider, DataAvailabilityProvider,
    PipelineResult,
};
use super::MantleBlobSource;
use alloc::{boxed::Box, fmt::Debug};
use alloy_primitives::{Address, Bytes};
use async_trait::async_trait;
use kona_genesis::RollupConfig;
use kona_protocol::BlockInfo;

/// A factory for creating an Ethereum data source provider with Mantle Arsia hardfork support.
#[derive(Debug, Clone)]
pub struct MantleEthereumDataSource<C, B>
where
    C: ChainProvider + Send + Clone,
    B: BlobProvider + Send + Clone,
{
    /// The ecotone timestamp.
    pub ecotone_timestamp: Option<u64>,
    /// The Mantle Arsia timestamp.
    pub mantle_arsia_timestamp: Option<u64>,
    /// The Mantle blob source (used before Mantle Arsia).
    pub mantle_blob_source: MantleBlobSource<C, B>,
    /// The standard blob source (used after Mantle Arsia).
    pub blob_source: BlobSource<C, B>,
    /// The calldata source.
    pub calldata_source: CalldataSource<C>,
}

impl<C, B> MantleEthereumDataSource<C, B>
where
    C: ChainProvider + Send + Clone + Debug,
    B: BlobProvider + Send + Clone + Debug,
{
    /// Instantiates a new [`MantleEthereumDataSource`].
    pub fn new(
        mantle_blob_source: MantleBlobSource<C, B>,
        blob_source: BlobSource<C, B>,
        calldata_source: CalldataSource<C>,
        cfg: &RollupConfig,
    ) -> Self {
        Self {
            ecotone_timestamp: cfg.hardforks.ecotone_time,
            mantle_arsia_timestamp: cfg.hardforks.mantle_arsia_time,
            mantle_blob_source,
            blob_source,
            calldata_source,
        }
    }

    /// Instantiates a new [`MantleEthereumDataSource`] from parts.
    pub fn new_from_parts(provider: C, blobs: B, cfg: &RollupConfig) -> Self {
        Self {
            ecotone_timestamp: cfg.hardforks.ecotone_time,
            mantle_arsia_timestamp: cfg.hardforks.mantle_arsia_time,
            mantle_blob_source: MantleBlobSource::new(
                provider.clone(),
                blobs.clone(),
                cfg.batch_inbox_address,
            ),
            blob_source: BlobSource::new(provider.clone(), blobs, cfg.batch_inbox_address),
            calldata_source: CalldataSource::new(provider, cfg.batch_inbox_address),
        }
    }
}

#[async_trait]
impl<C, B> DataAvailabilityProvider for MantleEthereumDataSource<C, B>
where
    C: ChainProvider + Send + Sync + Clone + Debug,
    B: BlobProvider + Send + Sync + Clone + Debug,
{
    type Item = Bytes;

    async fn next(
        &mut self,
        block_ref: &BlockInfo,
        batcher_address: Address,
    ) -> PipelineResult<Self::Item> {
        let ecotone_enabled =
            self.ecotone_timestamp.map(|e| block_ref.timestamp >= e).unwrap_or(false);
        if ecotone_enabled {
            // Check if Mantle Arsia hardfork is active
            let mantle_arsia_enabled = self
                .mantle_arsia_timestamp
                .map(|t| block_ref.timestamp >= t)
                .unwrap_or(false);

            if mantle_arsia_enabled {
                // After Mantle Arsia: use standard blob decoding
                self.blob_source.next(block_ref, batcher_address).await
            } else {
                // Before Mantle Arsia: use Mantle blob decoding
                self.mantle_blob_source.next(block_ref, batcher_address).await
            }
        } else {
            self.calldata_source.next(block_ref, batcher_address).await
        }
    }

    fn clear(&mut self) {
        self.mantle_blob_source.clear();
        self.blob_source.clear();
        self.calldata_source.clear();
    }
}

