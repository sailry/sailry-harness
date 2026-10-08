//! Batch processing for multiple generation requests.

use snafu::Snafu;

/// Batch request builder.
pub mod builder;
pub use builder::BatchBuilder;
/// Batch operation handle for lifecycle management.
pub mod handle;
pub use handle::*;
/// Wire types for batch API requests and responses.
pub mod model;

/// Errors that can occur during batch operations.
#[derive(Debug, Snafu)]
pub enum Error {
    /// An error from the underlying Gemini client.
    Client {
        /// The underlying client error.
        source: crate::client::Error,
    },
    /// An error from file operations.
    File {
        /// The underlying file error.
        source: crate::files::Error,
    },
    /// A serialization error.
    Serialize {
        /// The underlying serde_json error.
        source: serde_json::Error,
    },
}
