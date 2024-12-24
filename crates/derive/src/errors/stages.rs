//! Error types for derivation pipeline stages.

use op_alloy_protocol::MAX_RLP_BYTES_PER_CHANNEL;
use thiserror::Error;

/// A frame decompression error.
#[derive(Error, Debug, PartialEq, Eq)]
pub enum BatchDecompressionError {
    /// The buffer exceeds the [MAX_RLP_BYTES_PER_CHANNEL] protocol parameter.
    #[error("The batch exceeds the maximum number of elements: {max_size}", max_size = MAX_RLP_BYTES_PER_CHANNEL)]
    BatchTooLarge,
}
