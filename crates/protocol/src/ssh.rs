use crate::{Secret, SshId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Authentication {
    Password,
    PrivateKey,
    KeyPath,
    Agent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Credential {
    Password {
        password: Secret,
    },
    PrivateKey {
        key: Secret,
        passphrase: Option<Secret>,
    },
    KeyPath {
        path: Secret,
        passphrase: Option<Secret>,
    },
    Agent,
}

impl Credential {
    pub fn authentication(&self) -> Authentication {
        match self {
            Self::Password { .. } => Authentication::Password,
            Self::PrivateKey { .. } => Authentication::PrivateKey,
            Self::KeyPath { .. } => Authentication::KeyPath,
            Self::Agent => Authentication::Agent,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostKey {
    pub algorithm: String,
    pub fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalLaunch {
    pub viewport: crate::terminal::Viewport,
    pub appearance: crate::terminal::Appearance,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sharing: Option<crate::connection::Sharing>,
    pub id: SshId,
    pub revision: u64,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub authentication: Authentication,
    pub host_key: Option<HostKey>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    FilesChanged,
    Directory(Directory),
    Download(Download),
    HostKeyRequired {
        key: HostKey,
        changed: bool,
    },
    HostInstalled {
        node: crate::NodeId,
    },
    Connected,
    Terminal(crate::terminal::Info),
    Transferred {
        bytes: u64,
    },
    Completed {
        exit_code: u32,
        stdout: String,
        stderr: String,
        truncated: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Upload,
    Download,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transfer {
    pub worktree: crate::WorktreeId,
    /// Relative to the worktree on the execution Node.
    pub path: String,
    pub remote_path: String,
    pub direction: Direction,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum InstallProgress {
    Connecting,
    Detecting,
    Uploading { sent: u64, total: u64 },
    Installing,
    Pairing,
}

/// Remote paths are interpreted by the SSH server, never as local Node paths.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Directory {
    pub path: String,
    pub entries: Vec<crate::FileEntry>,
    pub next: Option<Cursor>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub directory: bool,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadSpec {
    pub profile: crate::SshId,
    pub expected_revision: u64,
    pub path: String,
    pub size: u64,
    pub revision: String,
    pub overwrite: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Upload {
    pub stream: crate::StreamId,
    pub spec: UploadSpec,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Download {
    pub stream: crate::StreamId,
    pub path: String,
    pub size: u64,
    pub revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum FileAction {
    Create { directory: bool },
    Copy { destination: String },
    Move { destination: String },
    Remove,
}
