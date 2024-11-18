//! This module contains the `BatchStream` stage.

use crate::{
    errors::{PipelineEncodingError, PipelineError},
    stages::NextBatchProvider,
    traits::{L2ChainProvider, OriginAdvancer, OriginProvider, SignalReceiver},
    types::{PipelineResult, Signal},
};
use alloc::{boxed::Box, collections::VecDeque, sync::Arc};
use async_trait::async_trait;
use core::fmt::Debug;
use op_alloy_genesis::RollupConfig;
use op_alloy_protocol::{
    Batch, BatchValidity, BatchWithInclusionBlock, BlockInfo, L2BlockInfo, SingleBatch,
};

/// Provides [Batch]es for the [BatchStream] stage.
#[async_trait]
pub trait BatchStreamProvider {
    /// Returns the next [Batch] in the [BatchStream] stage.
    async fn next_batch(&mut self) -> PipelineResult<Batch>;

}

/// [BatchStream] stage in the derivation pipeline.
///
/// This stage is introduced in the [Holocene] hardfork.
/// It slots in between the [ChannelReader] and [BatchQueue]
/// stages, buffering span batches until they are validated.
///
/// [Holocene]: https://specs.optimism.io/protocol/holocene/overview.html
/// [ChannelReader]: crate::stages::ChannelReader
/// [BatchQueue]: crate::stages::BatchQueue
#[derive(Debug)]
pub struct BatchStream<P>
where
    P: BatchStreamProvider + OriginAdvancer + OriginProvider + SignalReceiver + Debug,
{
    /// The previous stage in the derivation pipeline.
    prev: P,

}

impl<P> BatchStream<P>
where
    P: BatchStreamProvider + OriginAdvancer + OriginProvider + SignalReceiver + Debug,
{
    /// Create a new [BatchStream] stage.
    pub const fn new(prev: P) -> Self {
        Self { prev }
    }

}

#[async_trait]
impl<P> NextBatchProvider for BatchStream<P>
where
    P: BatchStreamProvider + OriginAdvancer + OriginProvider + SignalReceiver + Send + Debug,
{
    async fn next_batch(
        &mut self,
    ) -> PipelineResult<Batch> {
        self.prev.next_batch().await
    }
}

#[async_trait]
impl<P> OriginAdvancer for BatchStream<P>
where
    P: BatchStreamProvider + OriginAdvancer + OriginProvider + SignalReceiver + Send + Debug,
{
    async fn advance_origin(&mut self) -> PipelineResult<()> {
        self.prev.advance_origin().await
    }
}

impl<P> OriginProvider for BatchStream<P>
where
    P: BatchStreamProvider + OriginAdvancer + OriginProvider + SignalReceiver + Debug,
{
    fn origin(&self) -> Option<BlockInfo> {
        self.prev.origin()
    }
}

#[async_trait]
impl<P> SignalReceiver for BatchStream<P>
where
    P: BatchStreamProvider + OriginAdvancer + OriginProvider + SignalReceiver + Debug + Send,
{
    async fn signal(&mut self, signal: Signal) -> PipelineResult<()> {
        self.prev.signal(signal).await?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        test_utils::{CollectingLayer, TestBatchStreamProvider, TestL2ChainProvider, TraceStorage},
        types::ResetSignal,
    };
    use alloc::vec;
    use op_alloy_protocol::{SingleBatch};
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};


    #[tokio::test]
    async fn test_batch_stream_reset() {
        let config = Arc::new(RollupConfig { ..RollupConfig::default() });
        let prev = TestBatchStreamProvider::new(vec![]);
        let mut stream = BatchStream::new(prev);
        assert!(!stream.prev.reset);
        stream.signal(ResetSignal::default().signal()).await.unwrap();
        assert!(stream.prev.reset);
    }

    #[tokio::test]
    async fn test_batch_stream_flush_channel() {
        let config = Arc::new(RollupConfig { ..RollupConfig::default() });
        let prev = TestBatchStreamProvider::new(vec![]);
        let mut stream = BatchStream::new(prev);
        assert!(!stream.prev.flushed);
        stream.signal(Signal::FlushChannel).await.unwrap();
        assert!(stream.prev.flushed);
    }


    #[tokio::test]
    async fn test_single_batch_pass_through() {
        let data = vec![Ok(Batch::Single(SingleBatch::default()))];
        let config = Arc::new(RollupConfig { ..RollupConfig::default() });
        let prev = TestBatchStreamProvider::new(data);
        let mut stream = BatchStream::new(prev);


        // The next batch should be passed through to the [BatchQueue] stage.
        let batch = stream.next_batch().await.unwrap();
        assert!(matches!(batch, Batch::Single(_)));
    }
}
