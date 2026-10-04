use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("a Node already owns profile {0}")]
    ProfileInUse(PathBuf),
    #[error("invalid profile {path}: {reason}")]
    InvalidProfile { path: PathBuf, reason: &'static str },
    #[error("Node service is stopping or stopped")]
    Stopped,
    #[error("Node task capacity is exhausted")]
    Busy,
    #[error("Node worker failed: {0}")]
    Worker(String),
    #[error("Node shutdown deadline elapsed; remaining tasks were cancelled")]
    ShutdownTimeout,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
    #[error(transparent)]
    Link(#[from] sailry_protocol::Fault),
}
