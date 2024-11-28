//! Errors for the `kona-executor` crate.

use alloc::string::String;
use kona_mpt::TrieNodeError;
use revm::primitives::EVMError;
use thiserror::Error;

/// The error type for the [StatelessL2BlockExecutor].
///
/// [StatelessL2BlockExecutor]: crate::StatelessL2BlockExecutor
#[derive(Error, Debug)]
pub enum ExecutorError {
    /// Missing gas limit in the payload attributes.
    #[error("Gas limit not provided in payload attributes")]
    MissingGasLimit,
    /// Missing transactions in the payload attributes.
    #[error("Transactions not provided in payload attributes")]
    MissingTransactions,
    /// Missing EIP-1559 parameters in execution payload post-Holocene.
    #[error("Missing EIP-1559 parameters in execution payload post-Holocene")]
    MissingEIP1559Params,
    /// Missing parent beacon block root in the payload attributes.
    #[error("Parent beacon block root not provided in payload attributes")]
    MissingParentBeaconBlockRoot,
    /// Invalid `extraData` field in the block header.
    #[error("Invalid `extraData` field in the block header")]
    InvalidExtraData,
    /// Block gas limit exceeded.
    #[error("Block gas limit exceeded")]
    BlockGasLimitExceeded,
    /// Unsupported transaction type.
    #[error("Unsupported transaction type: {_0}")]
    UnsupportedTransactionType(u8),
    /// Trie DB error.
    #[error("Trie error: {_0}")]
    TrieDBError(#[from] TrieDBError),
    /// Execution error.
    #[error("Execution error: {_0}")]
    ExecutionError(EVMError<TrieDBError>),
    /// Signature error.
    #[error("Signature error: {_0}")]
    SignatureError(alloy_primitives::SignatureError),
    /// RLP error.
    #[error("RLP error: {_0}")]
    RLPError(alloy_eips::eip2718::Eip2718Error),
}


/// A [Result] type for the [ExecutorError] enum.
pub type ExecutorResult<T> = Result<T, ExecutorError>;

/// A [Result] type alias where the error is [TrieDBError].
pub type TrieDBResult<T> = Result<T, TrieDBError>;

/// An error type for [TrieDB] operations.
///
/// [TrieDB]: crate::TrieDB
#[derive(Error, Debug, PartialEq, Eq)]
pub enum TrieDBError {
    /// Trie root node has not been blinded.
    #[error("Trie root node has not been blinded")]
    RootNotBlinded,
    /// Missing account info for bundle account.
    #[error("Missing account info for bundle account.")]
    MissingAccountInfo,
    /// Trie node error.
    #[error("Trie node error: {_0}")]
    TrieNode(#[from] TrieNodeError),
    /// Trie provider error.
    #[error("Trie provider error: {_0}")]
    Provider(String),
}

