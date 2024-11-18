//! This module contains the [BatchProvider] stage.

use super::NextBatchProvider;
use crate::{
    errors::{PipelineError, PipelineResult},
    stages::{BatchQueue},
    traits::{
        AttributesProvider, L2ChainProvider, OriginAdvancer, OriginProvider, Signal, SignalReceiver,
    },
};
use alloc::{boxed::Box, sync::Arc};
use async_trait::async_trait;
use core::fmt::Debug;
use op_alloy_genesis::RollupConfig;
use op_alloy_protocol::{BlockInfo, L2BlockInfo, SingleBatch};

/// The [BatchProvider] stage is a mux between the [BatchQueue] and [BatchValidator] stages.
///
/// Rules:
/// When Holocene is not active, the [BatchQueue] is used.
/// When Holocene is active, the [BatchValidator] is used.
///
/// When transitioning between the two stages, the mux will reset the active stage, but
/// retain `l1_blocks`.
#[derive(Debug)]
pub struct BatchProvider<P>
where
    P: NextBatchProvider + OriginAdvancer + OriginProvider + SignalReceiver + Debug,
{
    /// The rollup configuration.
    cfg: Arc<RollupConfig>,
    /// The previous stage of the derivation pipeline.
    ///
    /// If this is set to [None], the multiplexer has been activated and the active stage
    /// owns the previous stage.
    ///
    /// Must be [None] if `batch_queue` or `batch_validator` is [Some].
    prev: Option<P>,
    /// The batch queue stage of the provider.
    ///
    /// Must be [None] if `prev` or `batch_validator` is [Some].
    batch_queue: Option<BatchQueue<P>>,
}

impl<P> BatchProvider<P>
where
    P: NextBatchProvider + OriginAdvancer + OriginProvider + SignalReceiver + Debug,
{
    /// Creates a new [BatchProvider] with the given configuration and previous stage.
    pub const fn new(cfg: Arc<RollupConfig>, prev: P) -> Self {
        Self { cfg, prev: Some(prev), batch_queue: None }
    }

    /// Attempts to update the active stage of the mux.
    pub(crate) fn attempt_update(&mut self) -> PipelineResult<()> {
        if let Some(prev) = self.prev.take() {
            // On the first call to `attempt_update`, we need to determine the active stage to
            // initialize the mux with.
            self.batch_queue =
                    Some(BatchQueue::new(self.cfg.clone(), prev));
        }
        Ok(())
    }
}

#[async_trait]
impl<P> OriginAdvancer for BatchProvider<P>
where
    P: NextBatchProvider + OriginAdvancer + OriginProvider + SignalReceiver + Send + Debug,
{
    async fn advance_origin(&mut self) -> PipelineResult<()> {
        self.attempt_update()?;

        if let Some(batch_queue) = self.batch_queue.as_mut() {
            batch_queue.advance_origin().await
        } else {
            Err(PipelineError::NotEnoughData.temp())
        }
    }
}

impl<P> OriginProvider for BatchProvider<P>
where
    P: NextBatchProvider + OriginAdvancer + OriginProvider + SignalReceiver + Debug,
{
    fn origin(&self) -> Option<BlockInfo> {
        self.batch_queue.as_ref().map_or_else(
            || self.prev.as_ref().and_then(|prev| prev.origin()),
            |batch_queue| batch_queue.origin(),
        )
    }
}

#[async_trait]
impl<P> SignalReceiver for BatchProvider<P>
where
    P: NextBatchProvider + OriginAdvancer + OriginProvider + SignalReceiver + Send + Debug,
{
    async fn signal(&mut self, signal: Signal) -> PipelineResult<()> {
        self.attempt_update()?;

         if let Some(batch_queue) = self.batch_queue.as_mut() {
            batch_queue.signal(signal).await
        } else {
            Err(PipelineError::NotEnoughData.temp())
        }
    }
}

#[async_trait]
impl<P> AttributesProvider for BatchProvider<P>
where
    P: NextBatchProvider + OriginAdvancer + OriginProvider + SignalReceiver + Debug + Send,
{

    async fn next_batch(&mut self, parent: L2BlockInfo) -> PipelineResult<SingleBatch> {
        self.attempt_update()?;

        if let Some(batch_queue) = self.batch_queue.as_mut() {
            batch_queue.next_batch(parent).await
        } else {
            Err(PipelineError::NotEnoughData.temp())
        }
    }
}

#[cfg(test)]
mod test {
    use super::BatchProvider;
    use crate::{
        test_utils::{TestL2ChainProvider, TestNextBatchProvider},
        traits::{OriginProvider, ResetSignal, SignalReceiver},
    };
    use alloc::{sync::Arc, vec};
    use op_alloy_genesis::RollupConfig;
    use op_alloy_protocol::BlockInfo;

    #[test]
    fn test_batch_provider_validator_active() {
        let provider = TestNextBatchProvider::new(vec![]);
        let l2_provider = TestL2ChainProvider::default();
        let cfg = Arc::new(RollupConfig {  ..Default::default() });
        let mut batch_provider = BatchProvider::new(cfg, provider);

        assert!(batch_provider.attempt_update().is_ok());
        assert!(batch_provider.prev.is_none());
        assert!(batch_provider.batch_queue.is_none());
    }

    #[test]
    fn test_batch_provider_batch_queue_active() {
        let provider = TestNextBatchProvider::new(vec![]);
        let l2_provider = TestL2ChainProvider::default();
        let cfg = Arc::new(RollupConfig::default());
        let mut batch_provider = BatchProvider::new(cfg, provider);

        assert!(batch_provider.attempt_update().is_ok());
        assert!(batch_provider.prev.is_none());
        assert!(batch_provider.batch_queue.is_some());
    }

    #[test]
    fn test_batch_provider_transition_stage() {
        let provider = TestNextBatchProvider::new(vec![]);
        let l2_provider = TestL2ChainProvider::default();
        let cfg = Arc::new(RollupConfig { ..Default::default() });
        let mut batch_provider = BatchProvider::new(cfg, provider);

        batch_provider.attempt_update().unwrap();

        // Update the L1 origin to Holocene activation.
        let Some(ref mut stage) = batch_provider.batch_queue else {
            panic!("Expected BatchQueue");
        };
        stage.prev.origin = Some(BlockInfo { number: 1, timestamp: 2, ..Default::default() });

        // Transition to the BatchValidator stage.
        batch_provider.attempt_update().unwrap();
        assert!(batch_provider.batch_queue.is_none());

        assert_eq!(batch_provider.origin().unwrap().number, 1);
    }

    #[test]
    fn test_batch_provider_transition_stage_backwards() {
        let provider = TestNextBatchProvider::new(vec![]);
        let l2_provider = TestL2ChainProvider::default();
        let cfg = Arc::new(RollupConfig { ..Default::default() });
        let mut batch_provider = BatchProvider::new(cfg, provider);

        batch_provider.attempt_update().unwrap();

        // Update the L1 origin to Holocene activation.
        let Some(ref mut stage) = batch_provider.batch_queue else {
            panic!("Expected BatchQueue");
        };
        stage.prev.origin = Some(BlockInfo { number: 1, timestamp: 2, ..Default::default() });

        // Transition to the BatchValidator stage.
        batch_provider.attempt_update().unwrap();
        assert!(batch_provider.batch_queue.is_none());

        stage.prev.origin = Some(BlockInfo::default());

        batch_provider.attempt_update().unwrap();
        assert!(batch_provider.batch_queue.is_some());
    }

    #[tokio::test]
    async fn test_batch_provider_reset_bq() {
        let provider = TestNextBatchProvider::new(vec![]);
        let l2_provider = TestL2ChainProvider::default();
        let cfg = Arc::new(RollupConfig::default());
        let mut batch_provider = BatchProvider::new(cfg, provider);

        // Reset the batch provider.
        batch_provider.signal(ResetSignal::default().signal()).await.unwrap();

        let Some(bq) = batch_provider.batch_queue else {
            panic!("Expected BatchQueue");
        };
        assert!(bq.l1_blocks.len() == 1);
    }

    #[tokio::test]
    async fn test_batch_provider_reset_validator() {
        let provider = TestNextBatchProvider::new(vec![]);
        let l2_provider = TestL2ChainProvider::default();
        let cfg = Arc::new(RollupConfig { ..Default::default() });
        let mut batch_provider = BatchProvider::new(cfg, provider);

        // Reset the batch provider.
        batch_provider.signal(ResetSignal::default().signal()).await.unwrap();

    }
}
