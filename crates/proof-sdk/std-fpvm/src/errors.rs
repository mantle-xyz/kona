//! Errors for the `kona-std-fpvm` crate.

use thiserror::Error;

<<<<<<<< HEAD:crates/proof-sdk/std-fpvm/src/errors.rs
/// An error that can occur when reading from or writing to a file descriptor.
#[derive(Error, Debug, PartialEq, Eq)]
#[error("IO error (errno: {_0})")]
pub struct IOError(pub i32);

========
use thiserror::Error;

/// An error that can occur when reading from or writing to a file descriptor.
#[derive(Error, Debug, PartialEq, Eq)]
#[error("IO error (errno: {_0})")]
pub struct IOError(pub i32);

>>>>>>>> bc1ac67 (Refactor code structure; update alloy package version; add new features.):crates/common/src/errors.rs
/// A [Result] type for the [IOError].
pub type IOResult<T> = Result<T, IOError>;
