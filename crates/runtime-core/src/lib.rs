//! Node errors, profile ownership, workspace layout and confined file primitives
//! shared by the Node runtime and its extracted service crates.
mod error;
pub mod files;
pub mod profile;
pub mod workspaces;

pub use error::Error;
