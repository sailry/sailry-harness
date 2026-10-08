//! Content caching for reusable contexts and system instructions.

use snafu::Snafu;

/// Cache builder for creating cached content.
pub mod builder;
pub use builder::CacheBuilder;
/// Handle for managing cached content lifecycle.
pub mod handle;
pub use handle::CachedContentHandle;
/// Wire types for cache API requests and responses.
pub mod model;

/// Errors that can occur during cache operations.
#[derive(Debug, Snafu)]
pub enum Error {
    /// An error from the underlying Gemini client.
    #[snafu(display("client invocation error"))]
    Client {
        /// The underlying client error.
        source: Box<crate::client::Error>,
    },

    /// The display name exceeds the 128-character limit.
    #[snafu(display(
        "cache display name ('{display_name}') too long ({chars}), must be under 128 characters"
    ))]
    LongDisplayName {
        /// The display name that was too long.
        display_name: String,
        /// The number of characters in the display name.
        chars: usize,
    },

    /// Expiration (TTL or expire time) was not provided.
    #[snafu(display("expiration (TTL or expire time) is required for cache creation"))]
    MissingExpiration,
}
