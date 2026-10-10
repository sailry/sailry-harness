//! Explicit profile exports and restores to separately selected empty directories.
use crate::{Fault, NodeId, plugin::settings::SecretUpdate};
use serde::{Deserialize, Serialize};

/// Validate a newly chosen passphrase without restricting existing archive recovery.
pub fn validate_passphrase(value: &str) -> Result<(), Fault> {
    if value.chars().count() < 8 || value.len() > 8192 || value.chars().any(char::is_control) {
        return Err(Fault::invalid(
            "backup passphrase requires at least 8 characters without control characters",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Webdav,
    S3,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    pub kind: Kind,
    pub endpoint: String,
    pub root: String,
    pub bucket: String,
    pub region: String,
}

impl Destination {
    pub fn validate(&self) -> Result<(), Fault> {
        let url = url::Url::parse(&self.endpoint)
            .map_err(|_| Fault::invalid("invalid backup endpoint"))?;
        if self.endpoint.len() > 4096
            || !matches!(url.scheme(), "https" | "http")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(Fault::invalid(
                "backup endpoint requires an HTTP URL without credentials",
            ));
        }
        if self.root.len() > 4096
            || self.root.contains(['\\', ':'])
            || self.root.chars().any(char::is_control)
            || self.root.split('/').any(|part| matches!(part, "." | ".."))
        {
            return Err(Fault::invalid("invalid backup storage prefix"));
        }
        if self.kind == Kind::S3
            && (self.bucket.is_empty()
                || self.bucket.len() > 255
                || self
                    .bucket
                    .chars()
                    .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_')))
                || self.region.is_empty()
                || self.region.len() > 128
                || self
                    .region
                    .chars()
                    .any(|ch| !(ch.is_ascii_alphanumeric() || ch == '-')))
        {
            return Err(Fault::invalid("S3 bucket and region are required"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub revision: u64,
    pub destination: Option<Destination>,
    pub credentials_configured: bool,
    #[serde(default)]
    pub passphrase_configured: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub settings: Settings,
    /// WebDAV username or S3 access key ID; never returned by settings reads.
    pub identity: SecretUpdate,
    /// WebDAV password or S3 secret access key.
    pub secret: SecretUpdate,
    pub session_token: SecretUpdate,
    /// Encryption passphrase stays on the execution Node and is never returned.
    #[serde(default = "keep")]
    pub passphrase: SecretUpdate,
}

fn keep() -> SecretUpdate {
    SecretUpdate::Keep
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub id: String,
    pub node: NodeId,
    pub created_ms: u64,
    pub size_bytes: u64,
    pub files: u32,
    pub sha256: String,
}

impl Record {
    pub fn validate(&self) -> Result<(), Fault> {
        let id = uuid::Uuid::parse_str(&self.id)
            .map_err(|_| Fault::invalid("invalid backup identifier"))?;
        if id.to_string() != self.id
            || self.sha256.len() != 64
            || !self.sha256.bytes().all(|ch| ch.is_ascii_hexdigit())
        {
            return Err(Fault::invalid("invalid backup metadata"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub records: Vec<Record>,
    pub partial: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_missing_passphrase_state() {
        let settings: Settings = serde_json::from_value(serde_json::json!({
            "revision": 0,
            "destination": null,
            "credentials_configured": false,
        }))
        .unwrap();
        assert_eq!(settings, Settings::default());
        let update: Update = serde_json::from_value(serde_json::json!({
            "settings": settings,
            "identity": {"kind":"keep"},
            "secret": {"kind":"keep"},
            "session_token": {"kind":"keep"},
        }))
        .unwrap();
        assert_eq!(update.passphrase, SecretUpdate::Keep);
    }
    #[test]
    fn validates_storage_targets() {
        let mut destination = Destination {
            kind: Kind::S3,
            endpoint: "https://s3.example.com".into(),
            root: "profiles/backups".into(),
            bucket: "bucket".into(),
            region: "us-east-1".into(),
        };
        destination.validate().unwrap();
        for endpoint in [
            "https://user:secret@s3.example.com",
            "file:///tmp/backup",
            "https://s3.example.com?token=secret",
        ] {
            destination.endpoint = endpoint.into();
            assert!(destination.validate().is_err());
        }
        destination.endpoint = "https://s3.example.com".into();
        destination.root = "profiles/../private".into();
        assert!(destination.validate().is_err());
    }
}
