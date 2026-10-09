//! Manual profile backups use the execution Node's storage and durable requests.
pub(crate) mod archive;
mod git;
mod remote;
#[cfg(test)]
mod tests;

use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, Secret, backup::Destination};
use serde::{Deserialize, Serialize};

pub(crate) const MAX_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
pub(crate) const MAX_FILES: usize = 200_000;
pub(crate) const MAX_MANIFEST: usize = 32 * 1024 * 1024;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Auth {
    pub identity: Option<Secret>,
    pub secret: Option<Secret>,
    pub session_token: Option<Secret>,
}

#[derive(Clone)]
pub(crate) struct Plan {
    pub destination: Destination,
    pub auth: Auth,
}

pub(crate) use remote::{export, list, restore};

pub(crate) fn cancelled(cancel: &CancellationToken) -> Result<(), Fault> {
    if cancel.is_cancelled() {
        Err(Fault::new(ErrorCode::Cancelled, "profile backup cancelled"))
    } else {
        Ok(())
    }
}

pub(crate) fn io(error: impl std::fmt::Display) -> Fault {
    Fault::new(
        ErrorCode::Unavailable,
        format!("profile backup storage failed: {error}"),
    )
}
