//! Contains the `PipelineBuilder` object that is used to build a `DerivationPipeline`.

use crate::{
    pipeline::DerivationPipeline,
    stages::{
        AttributesQueue, BatchProvider, BatchStream, ChannelProvider, ChannelReader, FrameQueue,
        L1Retrieval, L1Traversal,
    },
    traits::{AttributesBuilder, ChainProvider, DataAvailabilityProvider, L2ChainProvider},
};
use alloc::sync::Arc;
use core::fmt::Debug;
use op_alloy_genesis::RollupConfig;
use op_alloy_protocol::BlockInfo;

type L1TraversalStage<P> = L1Traversal<P>;
type L1RetrievalStage<DAP, P> = L1Retrieval<DAP, L1TraversalStage<P>>;
type FrameQueueStage<DAP, P> = FrameQueue<L1RetrievalStage<DAP, P>>;
type ChannelProviderStage<DAP, P> = ChannelProvider<FrameQueueStage<DAP, P>>;
type ChannelReaderStage<DAP, P> = ChannelReader<ChannelProviderStage<DAP, P>>;
type BatchStreamStage<DAP, P> = BatchStream<ChannelReaderStage<DAP, P>>;
type BatchProviderStage<DAP, P> = BatchProvider<BatchStreamStage<DAP, P>>;
type AttributesQueueStage<DAP, P, B> = AttributesQueue<BatchProviderStage<DAP, P>, B>;

/// The `PipelineBuilder` constructs a [DerivationPipeline] using a builder pattern.
#[derive(Debug)]
pub struct PipelineBuilder<B, P, T, D>
where
    B: AttributesBuilder + Send + Debug,
    P: ChainProvider + Send + Sync + Debug,
    T: L2ChainProvider + Clone + Send + Sync + Debug,
    D: DataAvailabilityProvider + Send + Sync + Debug,
{
    l2_chain_provider: Option<T>,
    dap_source: Option<D>,
    chain_provider: Option<P>,
    builder: Option<B>,
    origin: Option<BlockInfo>,
    rollup_config: Option<Arc<RollupConfig>>,
}

impl<B, P, T, D> Default for PipelineBuilder<B, P, T, D>
where
    B: AttributesBuilder + Send + Debug,
    P: ChainProvider + Send + Sync + Debug,
    T: L2ChainProvider + Clone + Send + Sync + Debug,
    D: DataAvailabilityProvider + Send + Sync + Debug,
{
    fn default() -> Self {
        Self {
            l2_chain_provider: None,
            dap_source: None,
            chain_provider: None,
            builder: None,
            origin: None,
            rollup_config: None,
        }
    }
}

impl<B, P, T, D> PipelineBuilder<B, P, T, D>
where
    B: AttributesBuilder + Send + Debug,
    P: ChainProvider + Send + Sync + Debug,
    T: L2ChainProvider + Clone + Send + Sync + Debug,
    D: DataAvailabilityProvider + Send + Sync + Debug,
{
    /// Creates a new pipeline builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the rollup config for the pipeline.
    pub fn rollup_config(mut self, rollup_config: Arc<RollupConfig>) -> Self {
        self.rollup_config = Some(rollup_config);
        self
    }

    /// Sets the origin L1 block for the pipeline.
    pub const fn origin(mut self, origin: BlockInfo) -> Self {
        self.origin = Some(origin);
        self
    }

    /// Sets the data availability provider for the pipeline.
    pub fn dap_source(mut self, dap_source: D) -> Self {
        self.dap_source = Some(dap_source);
        self
    }

    /// Sets the builder for the pipeline.
    pub fn builder(mut self, builder: B) -> Self {
        self.builder = Some(builder);
        self
    }

    /// Sets the l2 chain provider for the pipeline.
    pub fn l2_chain_provider(mut self, l2_chain_provider: T) -> Self {
        self.l2_chain_provider = Some(l2_chain_provider);
        self
    }

    /// Sets the chain provider for the pipeline.
    pub fn chain_provider(mut self, chain_provider: P) -> Self {
        self.chain_provider = Some(chain_provider);
        self
    }

    /// Builds the pipeline.
    pub fn build(self) -> DerivationPipeline<AttributesQueueStage<D, P, B>, T> {
        self.into()
    }
}

impl<B, P, T, D> From<PipelineBuilder<B, P, T, D>>
    for DerivationPipeline<AttributesQueueStage<D, P, B>, T>
where
    B: AttributesBuilder + Send + Debug,
    P: ChainProvider + Send + Sync + Debug,
    T: L2ChainProvider + Clone + Send + Sync + Debug,
    D: DataAvailabilityProvider + Send + Sync + Debug,
{
    fn from(builder: PipelineBuilder<B, P, T, D>) -> Self {
        // Extract the builder fields.
        let rollup_config = builder.rollup_config.expect("rollup_config must be set");
        let chain_provider = builder.chain_provider.expect("chain_provider must be set");
        let l2_chain_provider = builder.l2_chain_provider.expect("chain_provider must be set");
        let dap_source = builder.dap_source.expect("dap_source must be set");
        let attributes_builder = builder.builder.expect("builder must be set");

        // Compose the stage stack.
        let mut l1_traversal = L1Traversal::new(chain_provider, Arc::clone(&rollup_config));
        l1_traversal.block = Some(builder.origin.expect("origin must be set"));
        let l1_retrieval = L1Retrieval::new(l1_traversal, dap_source);
        let frame_queue = FrameQueue::new(l1_retrieval, Arc::clone(&rollup_config));
        let channel_provider = ChannelProvider::new(Arc::clone(&rollup_config), frame_queue);
        let channel_reader = ChannelReader::new(channel_provider, Arc::clone(&rollup_config));
        let batch_stream = BatchStream::new(channel_reader);
        let batch_provider = BatchProvider::new(rollup_config.clone(), batch_stream);
        let attributes =
            AttributesQueue::new(rollup_config.clone(), batch_provider, attributes_builder);

        // Create the pipeline.
        Self::new(attributes, rollup_config, l2_chain_provider)
    }
}
