//! This module contains derivation errors thrown within the pipeline.

use alloc::string::{String, ToString};
use alloy_eips::BlockNumHash;
use alloy_primitives::B256;
use op_alloy_genesis::system::SystemConfigUpdateError;
use op_alloy_protocol::{DepositError, SpanBatchError};

/// Blob Decuding Error
#[derive(derive_more::Display, Debug, PartialEq, Eq)]
pub enum BlobDecodingError {
    /// Invalid field element
    #[display("Invalid field element")]
    InvalidFieldElement,
    /// Invalid encoding version
    #[display("Invalid encoding version")]
    InvalidEncodingVersion,
    /// Invalid length
    #[display("Invalid length")]
    InvalidLength,
    /// Missing Data
    #[display("Missing data")]
    MissingData,
}

impl core::error::Error for BlobDecodingError {}

/// A result type for the derivation pipeline stages.
pub type PipelineResult<T> = Result<T, PipelineErrorKind>;

/// [crate::ensure] is a short-hand for bubbling up errors in the case of a condition not being met.
#[macro_export]
macro_rules! ensure {
    ($cond:expr, $err:expr) => {
        if !($cond) {
            return Err($err);
        }
    };
}

/// A top level filter for [PipelineError] that sorts by severity.
#[derive(derive_more::Display, Debug, PartialEq, Eq)]
pub enum PipelineErrorKind {
    /// A temporary error.
    #[display("Temporary error: {_0}")]
    Temporary(PipelineError),
    /// A critical error.
    #[display("Critical error: {_0}")]
    Critical(PipelineError),
    /// A reset error.
    #[display("Pipeline reset: {_0}")]
    Reset(ResetError),
}

impl From<ResetError> for PipelineErrorKind {
    fn from(err: ResetError) -> Self {
        Self::Reset(err)
    }
}

impl core::error::Error for PipelineErrorKind {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Temporary(err) => Some(err),
            Self::Critical(err) => Some(err),
            Self::Reset(err) => Some(err),
        }
    }
}

/// An error encountered during the processing.
#[derive(derive_more::Display, Debug, PartialEq, Eq)]
pub enum PipelineError {
    /// There is no data to read from the channel bank.
    #[display("EOF")]
    Eof,
    /// There is not enough data to complete the processing of the stage. If the operation is
    /// re-tried, more data will come in allowing the pipeline to progress, or eventually a
    /// [PipelineError::Eof] will be encountered.
    #[display("Not enough data")]
    NotEnoughData,
    /// No channels are available in the [ChannelProvider].
    ///
    /// [ChannelProvider]: crate::stages::ChannelProvider
    #[display("The channel provider is empty")]
    ChannelProviderEmpty,
    /// The channel has already been built by the [ChannelAssembler] stage.
    ///
    /// [ChannelAssembler]: crate::stages::ChannelAssembler
    #[display("Channel already built")]
    ChannelAlreadyBuilt,
    /// Failed to find channel in the [ChannelProvider].
    ///
    /// [ChannelProvider]: crate::stages::ChannelProvider
    #[display("Channel not found in channel provider")]
    ChannelNotFound,
    /// No channel returned by the [ChannelReader] stage.
    ///
    /// [ChannelReader]: crate::stages::ChannelReader
    #[display("The channel reader has no channel available")]
    ChannelReaderEmpty,
    /// The [BatchQueue] is empty.
    ///
    /// [BatchQueue]: crate::stages::BatchQueue
    #[display("The batch queue has no batches available")]
    BatchQueueEmpty,
    /// Missing L1 origin.
    #[display("Missing L1 origin from previous stage")]
    MissingOrigin,
    /// Missing data from [L1Retrieval].
    ///
    /// [L1Retrieval]: crate::stages::L1Retrieval
    #[display("L1 Retrieval missing data")]
    MissingL1Data,
    /// Invalid batch type passed.
    #[display("Invalid batch type passed to stage")]
    InvalidBatchType,
    /// Invalid batch validity variant.
    #[display("Invalid batch validity")]
    InvalidBatchValidity,
    /// [SystemConfig] update error.
    ///
    /// [SystemConfig]: op_alloy_genesis::SystemConfig
    #[display("Error updating system config: {_0}")]
    SystemConfigUpdate(SystemConfigUpdateError),
    /// Attributes builder error variant, with [BuilderError].
    #[display("Attributes builder error: {_0}")]
    AttributesBuilder(BuilderError),
    /// [PipelineEncodingError] variant.
    #[display("Decode error: {_0}")]
    BadEncoding(PipelineEncodingError),
    /// Provider error variant.
    #[display("Blob provider error: {_0}")]
    Provider(String),
    /// Found future batch
    #[display("Found batch with timestamp: {_0} marked as future batch, but expected timestamp: {_1}" )]
    FutureBatch(u64, u64),
    /// The data source can no longer provide any more data.
    #[display("Data source exhausted")]
    EndOfSource,
}

impl From<BuilderError> for PipelineError {
    fn from(err: BuilderError) -> Self {
        Self::AttributesBuilder(err)
    }
}

impl From<PipelineEncodingError> for PipelineError {
    fn from(err: PipelineEncodingError) -> Self {
        Self::BadEncoding(err)
    }
}

impl core::error::Error for PipelineError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::AttributesBuilder(err) => Some(err),
            Self::BadEncoding(err) => Some(err),
            _ => None,
        }
    }
}

impl PipelineError {
    /// Wrap [self] as a [PipelineErrorKind::Critical].
    pub const fn crit(self) -> PipelineErrorKind {
        PipelineErrorKind::Critical(self)
    }

    /// Wrap [self] as a [PipelineErrorKind::Temporary].
    pub const fn temp(self) -> PipelineErrorKind {
        PipelineErrorKind::Temporary(self)
    }
}

/// A reset error
#[derive(derive_more::Display, Clone, Debug, Eq, PartialEq)]
pub enum ResetError {
    /// The batch has a bad parent hash.
    /// The first argument is the expected parent hash, and the second argument is the actual
    /// parent hash.
    #[display("Bad parent hash: expected {_0}, got {_1}")]
    BadParentHash(B256, B256),
    /// The batch has a bad timestamp.
    /// The first argument is the expected timestamp, and the second argument is the actual
    /// timestamp.
    #[display("Bad timestamp: expected {_0}, got {_1}")]
    BadTimestamp(u64, u64),
    /// L1 origin mismatch.
    #[display("L1 origin mismatch. Expected {_0:?}, got {_1:?}")]
    L1OriginMismatch(u64, u64),
    /// The stage detected a block reorg.
    /// The first argument is the expected block hash.
    /// The second argument is the parent_hash of the next l1 origin block.
    #[display("L1 reorg detected: expected {_0}, got {_1}")]
    ReorgDetected(B256, B256),
    /// Attributes builder error variant, with [BuilderError].
    #[display("Attributes builder error: {_0}")]
    AttributesBuilder(BuilderError),
    /// A Holocene activation temporary error.
    #[display("Holocene activation reset")]
    HoloceneActivation,
}

impl From<BuilderError> for ResetError {
    fn from(err: BuilderError) -> Self {
        Self::AttributesBuilder(err)
    }
}

impl core::error::Error for ResetError {}

impl ResetError {
    /// Wrap [self] as a [PipelineErrorKind::Reset].
    pub const fn reset(self) -> PipelineErrorKind {
        PipelineErrorKind::Reset(self)
    }
}

/// A decoding error.
#[derive(derive_more::Display, Debug, PartialEq, Eq)]
pub enum PipelineEncodingError {
    /// The buffer is empty.
    #[display("Empty buffer")]
    EmptyBuffer,
    /// Deposit decoding error.
    #[display("Error decoding deposit: {_0}")]
    DepositError(DepositError),
    /// Alloy RLP Encoding Error.
    #[display("RLP error: {_0}")]
    AlloyRlpError(alloy_rlp::Error),
    /// Span Batch Error.
    #[display("{_0}")]
    SpanBatchError(SpanBatchError),
}

impl core::error::Error for PipelineEncodingError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::DepositError(err) => Some(err),
            Self::SpanBatchError(err) => Some(err),
            _ => None,
        }
    }
}

impl From<SpanBatchError> for PipelineEncodingError {
    fn from(err: SpanBatchError) -> Self {
        Self::SpanBatchError(err)
    }
}

impl From<DepositError> for PipelineEncodingError {
    fn from(err: DepositError) -> Self {
        Self::DepositError(err)
    }
}

/// A frame decompression error.
#[derive(derive_more::Display, Debug, PartialEq, Eq)]
pub enum BatchDecompressionError {
    /// The buffer exceeds the [MAX_SPAN_BATCH_ELEMENTS] protocol parameter.
    #[display("The batch exceeds the maximum number of elements: {max_size}", max_size = 10000000)]
    BatchTooLarge,
}

impl core::error::Error for BatchDecompressionError {}

/// An [AttributesBuilder] Error.
///
/// [AttributesBuilder]: crate::traits::AttributesBuilder
#[derive(derive_more::Display, Clone, Debug, PartialEq, Eq)]
pub enum BuilderError {
    /// Mismatched blocks.
    #[display("Block mismatch. Expected {_0:?}, got {_1:?}")]
    BlockMismatch(BlockNumHash, BlockNumHash),
    /// Mismatched blocks for the start of an Epoch.
    #[display("Block mismatch on epoch reset. Expected {_0:?}, got {_1:?}")]
    BlockMismatchEpochReset(BlockNumHash, BlockNumHash, B256),
    /// [SystemConfig] update failed.
    ///
    /// [SystemConfig]: op_alloy_genesis::SystemConfig
    #[display("System config update failed")]
    SystemConfigUpdate,
    /// Broken time invariant between L2 and L1.
    #[display("Time invariant broken. L1 origin: {_0:?} | Next L2 time: {_1} | L1 block: {_2:?} | L1 timestamp {_3:?}")]
    BrokenTimeInvariant(BlockNumHash, u64, BlockNumHash, u64),
    /// Attributes unavailable.
    #[display("Attributes unavailable")]
    AttributesUnavailable,
    /// A custom error.
    #[display("Error in attributes builder: {_0}")]
    Custom(String),
}

impl core::error::Error for BuilderError {}

/// An error returned by the [BlobProviderError].
#[derive(derive_more::Display, Debug, PartialEq, Eq)]
pub enum BlobProviderError {
    /// The number of specified blob hashes did not match the number of returned sidecars.
    #[display("Blob sidecar length mismatch: expected {_0}, got {_1}")]
    SidecarLengthMismatch(usize, usize),
    /// Slot derivation error.
    #[display("Failed to derive slot")]
    SlotDerivation,
    /// Blob decoding error.
    #[display("Blob decoding error: {_0}")]
    BlobDecoding(BlobDecodingError),
    /// Error pertaining to the backend transport.
    #[display("{_0}")]
    Backend(String),
}

impl From<BlobDecodingError> for BlobProviderError {
    fn from(err: BlobDecodingError) -> Self {
        Self::BlobDecoding(err)
    }
}

impl core::error::Error for BlobProviderError {}

/// An error returned by the [EigenDAProxyError]
#[derive(derive_more::Display, Debug, PartialEq, Eq)]
pub enum EigenDAProxyError {
    /// Retrieve blob error.
    #[display("Failed to retrieve blob, error: {_0}")]
    RetrieveBlob(String),
    /// Retrieve blob with commitment error.
    #[display("Failed to retrieve blob with commitment, error: {_0}")]
    RetrieveBlobWithCommitment(String),
    /// Disperse blob error.
    #[display("Failed to disperse blob, error: {_0}")]
    DisperseBlob(String),
    /// Get blob status error.
    #[display("Failed to get blob status, error: {_0}")]
    GetBlobStatus(String),
    /// No fund blob from EigenDA.
    #[display("Blob not fund from EigenDA")]
    NotFound,
    /// Invalid input data len.
    #[display("Invalid input data len for disperse blob from EigenDA")]
    InvalidInput,
    /// Request timeout.
    #[display("Request blob timeout, error: {_0}")]
    TimeOut(String),
}
impl core::error::Error for EigenDAProxyError {}


/// An error returned by the [EigenDAProviderError]
#[derive(derive_more::Display, Debug, PartialEq, Eq)]
pub enum EigenDAProviderError {
    /// Retrieve Frame from da indexer error.
    #[display("Failed to retrieve blob from da indexer, error: {_0}")]
    RetrieveFramesFromDaIndexer(String),
    /// Request timeout.
    #[display("Request blob timeout, error: {_0}")]
    TimeOut(String),
    /// Retrieve Frame from eigen da error.
    #[display("Failed to retrieve blob from eigen da, error: {_0}")]
    RetrieveBlob(String),
    #[display("Get blob from indexer da, status: {_0}")]
    Status(String),
    /// Error pertaining to the backend transport.
    #[display("{_0}")]
    Backend(String),
    #[display("Failed to decode blob, error: {_0}")]
    RLPDecodeError(String),
    #[display("Failed to decode proto buf, error: {_0}")]
    ProtoDecodeError(String),
    /// Retrieve Frame from blob error.
    #[display("Failed to retrieve blob from eth blob, error: {_0}")]
    Blob(String),
    #[display("Error: {_0}")]
    String(String),

}


impl core::error::Error for EigenDAProviderError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reset_error_kinds() {
        let reset_errors = [
            ResetError::BadParentHash(Default::default(), Default::default()),
            ResetError::BadTimestamp(0, 0),
            ResetError::L1OriginMismatch(0, 0),
            ResetError::ReorgDetected(Default::default(), Default::default()),
            ResetError::AttributesBuilder(BuilderError::BlockMismatch(
                Default::default(),
                Default::default(),
            )),
            ResetError::HoloceneActivation,
        ];
        for error in reset_errors.into_iter() {
            let expected = PipelineErrorKind::Reset(error.clone());
            assert_eq!(error.reset(), expected);
        }
    }
}
