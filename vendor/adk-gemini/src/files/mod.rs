//! File upload and management for the Gemini API.

use snafu::Snafu;

/// File upload builder.
pub mod builder;
/// File handle for managing uploaded files.
pub mod handle;
/// Wire types for file API requests and responses.
pub mod model;

/// Errors that can occur during file operations.
#[derive(Debug, Snafu)]
pub enum Error {
    /// An error from the underlying Gemini client.
    Client {
        /// The underlying client error.
        source: crate::client::Error,
    },
}
