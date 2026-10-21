//! CPU-only local decisions through the packaged, pinned llama.cpp C API.
mod engine;
mod ffi;
mod gguf;
mod inference;
mod runtime;

pub use engine::Engine;
pub use gguf::{Algorithm, Metadata, validate};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("decision input is invalid: {0}")]
    Invalid(&'static str),
    #[error("decision runtime is unavailable: {0}")]
    Unavailable(std::borrow::Cow<'static, str>),
    #[error("decision inference failed: {0}")]
    Native(&'static str),
    #[error("decision inference was cancelled")]
    Cancelled,
    #[error("decision file could not be read: {0}")]
    Io(#[from] std::io::Error),
    #[error("decision runtime could not load: {0}")]
    Library(#[from] libloading::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Checks the platform and the exact packaged runtime without loading a model.
pub fn available() -> Result<()> {
    runtime::available()
}
