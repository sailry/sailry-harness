//! Standalone Agent Skills imported into the execution Node's package inventory.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub repository: String,
    pub git_ref: Option<String>,
    pub path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolved {
    pub repository: String,
    pub git_ref: String,
    pub commit: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    /// Stable inventory identity for this source coordinate and skill directory.
    pub name: String,
    pub path: String,
    pub skill: super::Skill,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discovery {
    pub source: Resolved,
    pub skills: Vec<Candidate>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitProvenance {
    pub source: Resolved,
    pub path: String,
}

/// A Skillhub coordinate. The Node resolves an omitted namespace or version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistrySource {
    pub namespace: Option<String>,
    pub slug: String,
    pub version: Option<String>,
}

/// Immutable publisher and signed package identity, never a Git revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryResolved {
    pub namespace: String,
    pub slug: String,
    pub version: String,
    pub version_id: String,
    pub content_hash: String,
    pub package_md5: String,
    pub archive_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryDiscovery {
    pub source: RegistryResolved,
    pub skills: Vec<Candidate>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveFormat {
    Zip,
    Tar,
    TarGz,
}

/// An execution-Node snapshot, independent of controller paths and Git revisions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Imported {
    pub digest: String,
    pub path: String,
    pub kind: super::UploadSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportDiscovery {
    pub stream: crate::StreamId,
    pub digest: String,
    pub kind: super::UploadSource,
    pub skills: Vec<Candidate>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Provenance {
    Git(GitProvenance),
    Registry { registry: RegistryResolved },
    Imported { imported: Imported },
}

impl Provenance {
    pub fn git(&self) -> Option<&GitProvenance> {
        match self {
            Self::Git(source) => Some(source),
            Self::Registry { .. } | Self::Imported { .. } => None,
        }
    }
}
