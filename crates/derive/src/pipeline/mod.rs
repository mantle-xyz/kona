//! Module containing the derivation pipeline.

/// Re-export trait arguments.
pub use crate::traits::{
    AttributesBuilder, DataAvailabilityProvider, NextAttributes, OriginAdvancer, OriginProvider,
    Pipeline, ResetProvider,ResetSignal, Signal, SignalReceiver, StepResult,
};

/// Re-export error types.
pub use crate::errors::{PipelineError, PipelineResult};

mod builder;
pub use builder::PipelineBuilder;

mod core;
pub use core::DerivationPipeline;
