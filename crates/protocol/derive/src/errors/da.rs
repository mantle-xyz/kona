use alloc::string::String;
use thiserror::Error;

/// An error returned by the [EigenDAProxyError]
#[derive(Error, Debug, PartialEq, Eq)]
pub enum EigenDAProxyError {
    /// Retrieve blob error.
    #[error("Failed to retrieve blob, error: {0}")]
    RetrieveBlob(String),
    /// Retrieve blob with commitment error.
    #[error("Failed to retrieve blob with commitment, error: {0}")]
    RetrieveBlobWithCommitment(String),
    /// Get blob status error.
    #[error("Failed to get blob status, error: {0}")]
    GetBlobStatus(String),
    /// No fund blob from EigenDA.
    #[error("Blob not fund from EigenDA")]
    NotFound,
    /// Network error.
    #[error("Network error: {0}")]
    NetworkError(String),
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
    /// Get blob from indexer da status.
    #[error("Get blob from indexer da, status: {0}")]
    Status(String),
    /// Error pertaining to the backend transport.
    #[error("{0}")]
    Backend(String),
    /// Failed to decode RLP.
    #[error("Failed to decode RLP, error: {0}")]
    RLPDecodeError(String),
    /// Failed to decode proto buf.
    #[error("Failed to decode proto buf, error: {0}")]
    ProtoDecodeError(String),
}
