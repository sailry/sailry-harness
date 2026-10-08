//! Content generation including text, images, and audio.

/// Builder for content generation requests.
pub mod builder;
/// Wire types for generation requests and responses.
pub mod model;

pub use builder::ContentBuilder;
pub use model::*;
