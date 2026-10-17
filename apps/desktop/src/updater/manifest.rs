//! Authenticated v1 metadata; an asset digest is never treated as publisher authentication.
use super::{Failure, Result, config};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    path::{Component, Path},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u32,
    payload: Box<serde_json::value::RawValue>,
    signature: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    releases: Vec<Release>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Release {
    pub version: String,
    pub target: String,
    pub minimum_system: String,
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub bundle: String,
    pub executable: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Selection {
    pub manifest: Vec<u8>,
    pub channel: config::Channel,
    pub release: Release,
}

pub(super) struct Candidate {
    pub selection: Option<Selection>,
    pub has_release: bool,
    pub has_target: bool,
    pub incompatible: bool,
}

pub(super) fn parse(bytes: &[u8], keys: &[self_update::VerifyingKey]) -> Result<Vec<Release>> {
    if bytes.len() > config::MAX_MANIFEST || keys.is_empty() {
        return Err(Failure::new(
            "updates_manifest_invalid",
            "update metadata exceeds its limit or has no trusted keys",
        ));
    }
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(|_| {
        Failure::new(
            "updates_manifest_invalid",
            "invalid update metadata envelope",
        )
    })?;
    if envelope.version != 1 {
        return Err(Failure::new(
            "updates_manifest_invalid",
            "unsupported update metadata version",
        ));
    }
    let encoded = STANDARD.decode(&envelope.signature).map_err(|_| {
        Failure::new(
            "updates_signature_invalid",
            "invalid manifest signature encoding",
        )
    })?;
    let signature = ed25519_dalek::Signature::from_slice(&encoded).map_err(|_| {
        Failure::new(
            "updates_signature_invalid",
            "invalid manifest signature length",
        )
    })?;
    let verified = keys.iter().any(|bytes| {
        ed25519_dalek::VerifyingKey::from_bytes(bytes)
            .and_then(|key| key.verify_strict(envelope.payload.get().as_bytes(), &signature))
            .is_ok()
    });
    if !verified {
        return Err(Failure::new(
            "updates_signature_invalid",
            "update metadata was not signed by a trusted publisher",
        ));
    }
    let payload: Payload = serde_json::from_str(envelope.payload.get()).map_err(|_| {
        Failure::new(
            "updates_manifest_invalid",
            "invalid authenticated update metadata",
        )
    })?;
    let mut identities = std::collections::BTreeSet::new();
    for release in &payload.releases {
        release.validate()?;
        if !identities.insert((&release.version, &release.target)) {
            return Err(Failure::new(
                "updates_manifest_invalid",
                "duplicate update version and target",
            ));
        }
    }
    Ok(payload.releases)
}

pub(super) fn select(bytes: &[u8], config: &config::Config) -> Result<Option<Selection>> {
    finish(candidate(bytes, config)?)
}

pub(super) fn candidate(bytes: &[u8], config: &config::Config) -> Result<Candidate> {
    let releases = parse(bytes, &config.keys)?;
    let current = semver::Version::parse(&config.version).map_err(|_| {
        Failure::new(
            "updates_manifest_invalid",
            "invalid current application version",
        )
    })?;
    let system = numeric_version(&config.system)?;
    let mut result = Candidate {
        selection: None,
        has_release: !releases.is_empty(),
        has_target: false,
        incompatible: false,
    };
    for release in releases {
        if release.target != config.target {
            continue;
        }
        result.has_target = true;
        let version =
            semver::Version::parse(&release.version).expect("release version was validated");
        if config
            .required
            .as_ref()
            .is_some_and(|required| required != &release.version)
            || !config.channel.accepts(&version)
            || version.cmp_precedence(&current) != Ordering::Greater
        {
            continue;
        }
        if system < numeric_version(&release.minimum_system)? {
            result.incompatible = true;
            continue;
        }
        result.selection = prefer(
            result.selection,
            Selection {
                manifest: bytes.to_vec(),
                channel: config.channel,
                release,
            },
        );
    }
    Ok(result)
}

pub(super) fn finish(candidate: Candidate) -> Result<Option<Selection>> {
    if candidate.selection.is_some() {
        return Ok(candidate.selection);
    }
    if candidate.incompatible {
        return Err(Failure::new(
            "updates_system_old",
            "the update requires a newer operating system",
        ));
    }
    if candidate.has_release && !candidate.has_target {
        return Err(Failure::new(
            "updates_platform",
            "the manifest has no package for this target",
        ));
    }
    Ok(None)
}

pub(super) fn prefer(current: Option<Selection>, candidate: Selection) -> Option<Selection> {
    let Some(current) = current else {
        return Some(candidate);
    };
    let current_version = semver::Version::parse(&current.release.version)
        .expect("selected release version was validated");
    let candidate_version = semver::Version::parse(&candidate.release.version)
        .expect("selected release version was validated");
    if candidate_version.cmp_precedence(&current_version) == Ordering::Greater {
        Some(candidate)
    } else {
        Some(current)
    }
}

pub(super) fn revalidate(selection: &Selection, config: &config::Config) -> Result<()> {
    if selection.channel != config.channel {
        return Err(Failure::new(
            "updates_manifest_invalid",
            "the staged update channel changed",
        ));
    }
    let Some(current) = select(&selection.manifest, config)? else {
        return Err(Failure::new(
            "updates_version_old",
            "the selected update is not newer than this application",
        ));
    };
    if current.release != selection.release {
        return Err(Failure::new(
            "updates_manifest_invalid",
            "the staged update does not match its signed selection",
        ));
    }
    Ok(())
}

impl Release {
    fn validate(&self) -> Result<()> {
        semver::Version::parse(&self.version)
            .map_err(|_| Failure::new("updates_manifest_invalid", "invalid release version"))?;
        numeric_version(&self.minimum_system)?;
        config::validate_url(&self.url)?;
        let (bundle, executable) = layout(&self.target)?;
        if self.bundle != bundle
            || self.executable != executable
            || !safe_relative(&self.name)
            || Path::new(&self.name).components().count() != 1
            || !self.name.ends_with(".zip")
            || self.size == 0
            || self.size > config::MAX_ARCHIVE
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Failure::new(
                "updates_manifest_invalid",
                "invalid release package layout, size or digest",
            ));
        }
        Ok(())
    }
}

pub(super) fn layout(target: &str) -> Result<(&'static str, &'static str)> {
    match target {
        "aarch64-apple-darwin" | "x86_64-apple-darwin" => {
            Ok(("Sailry.app", "Contents/MacOS/sailry-desktop"))
        }
        "aarch64-pc-windows-msvc" | "x86_64-pc-windows-msvc" => {
            Ok(("Sailry", "sailry-desktop.exe"))
        }
        _ => Err(Failure::new(
            "updates_platform",
            "unsupported desktop update target",
        )),
    }
}

pub(super) fn safe_relative(value: &str) -> bool {
    !value.is_empty()
        && !value.contains(['\\', ':'])
        && Path::new(value)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn numeric_version(value: &str) -> Result<[u64; 4]> {
    let mut parts = [0; 4];
    let numbers: Vec<_> = value.split('.').collect();
    if numbers.is_empty() || numbers.len() > parts.len() {
        return Err(Failure::new(
            "updates_manifest_invalid",
            "invalid operating system version",
        ));
    }
    for (slot, number) in parts.iter_mut().zip(numbers) {
        if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(Failure::new(
                "updates_manifest_invalid",
                "invalid operating system version",
            ));
        }
        *slot = number.parse().map_err(|_| {
            Failure::new(
                "updates_manifest_invalid",
                "invalid operating system version",
            )
        })?;
    }
    Ok(parts)
}
