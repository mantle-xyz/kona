use alloc::string::String;
use thiserror::Error;

/// An [EigenDAProxyError] Error.

#[derive(Error, Debug, PartialEq, Eq)]
pub enum EigenDAProxyError {
    /// Retrieve blob error.
    #[error("Failed to retrieve blob, error: {0}")]
    RetrieveBlob(String),
    /// Retrieve blob with commitment error.
    #[error("Failed to retrieve blob with commitment, error: {0}")]
    RetrieveBlobWithCommitment(String),
    /// Disperse blob error.
    #[error("Failed to disperse blob, error: {0}")]
    DisperseBlob(String),
    /// Get blob status error.
    #[error("Failed to get blob status, error: {0}")]
    GetBlobStatus(String),
    /// No fund blob from EigenDA.
    #[error("Blob not fund from EigenDA")]
    NotFound,
    /// Invalid input data len.
    #[error("Invalid input data len for disperse blob from EigenDA")]
    InvalidInput,
    /// Request timeout.
    #[error("Request blob timeout, error: {0}")]
    TimeOut(String),
}

/// An error returned by the [EigenDAProviderError]
#[derive(Error, Debug, PartialEq, Eq)]
pub enum EigenDAProviderError {
    /// Retrieve Frame from da indexer error.
    #[error("Failed to retrieve blob from da indexer, error: {0}")]
    RetrieveFramesFromDaIndexer(String),
    /// Request timeout.
    #[error("Request blob timeout, error: {0}")]
    TimeOut(String),
    #[error("Get blob from indexer da, status: {0}")]
    Status(String),
    /// Error pertaining to the backend transport.
    #[error("{0}")]
    Backend(String),
    #[error("Failed to decode blob, error: {0}")]
    RLPDecodeError(String),
    #[error("Failed to decode proto buf, error: {0}")]
    ProtoDecodeError(String),

}