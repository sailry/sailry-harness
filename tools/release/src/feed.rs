use crate::{Result, archive, invalid};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer as _, SigningKey};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use sha2::{Digest as _, Sha256};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    path::Path,
};

const MAX_ARCHIVE: u64 = 2 * 1024 * 1024 * 1024;
const MAX_FEED: usize = 1024 * 1024;
const MACOS_MINIMUM: &str = "13.0";
const TARGETS: [&str; 2] = ["aarch64-apple-darwin", "x86_64-apple-darwin"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Experience,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u32,
    payload: Box<RawValue>,
    signature: String,
}

#[derive(Serialize)]
struct SignedEnvelope {
    version: u32,
    payload: Box<RawValue>,
    signature: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    releases: Vec<Release>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Release {
    pub(crate) version: String,
    pub(crate) target: String,
    pub(crate) minimum_system: String,
    pub(crate) name: String,
    pub(crate) url: String,
    pub(crate) sha256: String,
    pub(crate) size: u64,
    pub(crate) bundle: String,
    pub(crate) executable: String,
}

pub(crate) fn describe(
    path: &Path,
    version: &str,
    target: &str,
    minimum_system: &str,
    repository: &str,
    tag: &str,
    ring: &[[u8; 32]],
) -> Result<Release> {
    validate_repository(repository)?;
    validate_tag(version, tag)?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| invalid("update archive has no UTF-8 file name"))?
        .to_owned();
    let expected = format!("Sailry-{target}.zip");
    if name != expected {
        return Err(invalid("update archive name does not match its target").into());
    }
    archive::verify(path, &name, ring)?;
    let (sha256, size) = digest(path)?;
    let (bundle, executable) = layout(target)?;
    let release = Release {
        version: version.into(),
        target: target.into(),
        minimum_system: minimum_system.into(),
        url: format!("https://github.com/{repository}/releases/download/{tag}/{name}"),
        name,
        sha256,
        size,
        bundle: bundle.into(),
        executable: executable.into(),
    };
    release.validate(repository)?;
    Ok(release)
}

pub(crate) fn merge(
    current: &[u8],
    assets: &Path,
    channel: Channel,
    repository: &str,
    tag: &str,
    ring: &[[u8; 32]],
    signer: &SigningKey,
) -> Result<Vec<u8>> {
    validate_repository(repository)?;
    let version = tag
        .strip_prefix('v')
        .ok_or_else(|| invalid("release tag must start with v"))?;
    let release_version = parse_version(version)?;
    validate_tag(version, tag)?;
    if channel == Channel::Stable && !release_version.pre.is_empty() {
        return Err(invalid("prereleases cannot enter the stable update feed").into());
    }
    let mut releases = parse(current, repository, channel, ring)?;
    let additions = entries(assets, version, repository, ring)?;
    let mut identities: BTreeMap<(Version, String), Release> = releases
        .drain(..)
        .map(|release| {
            Ok((
                (parse_version(&release.version)?, release.target.clone()),
                release,
            ))
        })
        .collect::<Result<_>>()?;
    for release in additions {
        let identity = (parse_version(&release.version)?, release.target.clone());
        match identities.get(&identity) {
            Some(existing) if existing == &release => {}
            Some(_) => {
                return Err(invalid("published update metadata is immutable").into());
            }
            None => {
                identities.insert(identity, release);
            }
        }
    }
    let mut releases: Vec<_> = identities.into_values().collect();
    releases.sort_by(order);
    let bytes = sign(Payload { releases }, signer)?;
    let _ = parse(&bytes, repository, channel, ring)?;
    Ok(bytes)
}

fn entries(
    assets: &Path,
    version: &str,
    repository: &str,
    ring: &[[u8; 32]],
) -> Result<Vec<Release>> {
    let mut metadata = Vec::new();
    let mut archives = BTreeSet::new();
    for entry in fs::read_dir(assets)? {
        let path = entry?.path();
        if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
            if name.ends_with(".zip.update.json") {
                metadata.push(path);
            } else if name.ends_with(".zip") {
                archives.insert(name.to_owned());
            }
        }
    }
    metadata.sort();
    let mut targets = BTreeSet::new();
    let mut releases = Vec::new();
    for path in metadata {
        let bytes = fs::read(&path)?;
        let release: Release = serde_json::from_slice(&bytes)
            .map_err(|_| invalid("invalid release update metadata"))?;
        release.validate(repository)?;
        if release.version != version {
            return Err(invalid("release metadata version does not match its tag").into());
        }
        let metadata_name = path.file_name().and_then(|value| value.to_str());
        let expected_metadata = format!("{}.update.json", release.name);
        if metadata_name != Some(expected_metadata.as_str()) {
            return Err(invalid("release metadata name does not match its archive").into());
        }
        if !targets.insert(release.target.clone()) {
            return Err(invalid("release contains duplicate update targets").into());
        }
        let archive_path = assets.join(&release.name);
        archive::verify(&archive_path, &release.name, ring)?;
        let (sha256, size) = digest(&archive_path)?;
        if release.sha256 != sha256 || release.size != size {
            return Err(invalid("release archive differs from its update metadata").into());
        }
        releases.push(release);
    }
    let expected: BTreeSet<_> = TARGETS.into_iter().map(str::to_owned).collect();
    let described: BTreeSet<_> = releases
        .iter()
        .map(|release| release.name.clone())
        .collect();
    if targets != expected || archives != described {
        return Err(invalid("published release requires both macOS update targets").into());
    }
    Ok(releases)
}

fn parse(
    bytes: &[u8],
    repository: &str,
    channel: Channel,
    ring: &[[u8; 32]],
) -> Result<Vec<Release>> {
    if bytes.len() > MAX_FEED || ring.is_empty() {
        return Err(invalid("update feed exceeds its limit or has no trusted keys").into());
    }
    let envelope: Envelope =
        serde_json::from_slice(bytes).map_err(|_| invalid("invalid update feed envelope"))?;
    if envelope.version != 1 {
        return Err(invalid("unsupported update feed version").into());
    }
    let signature = STANDARD
        .decode(&envelope.signature)
        .map_err(|_| invalid("invalid update feed signature encoding"))?;
    let signature = ed25519_dalek::Signature::from_slice(&signature)
        .map_err(|_| invalid("invalid update feed signature length"))?;
    if !ring.iter().any(|bytes| {
        ed25519_dalek::VerifyingKey::from_bytes(bytes)
            .and_then(|key| key.verify_strict(envelope.payload.get().as_bytes(), &signature))
            .is_ok()
    }) {
        return Err(invalid("update feed has an unknown publisher").into());
    }
    let payload: Payload = serde_json::from_str(envelope.payload.get())
        .map_err(|_| invalid("invalid authenticated update feed"))?;
    let mut identities = BTreeSet::new();
    for release in &payload.releases {
        release.validate(repository)?;
        let version = parse_version(&release.version)?;
        if channel == Channel::Stable && !version.pre.is_empty() {
            return Err(invalid("stable update feed contains a prerelease").into());
        }
        if !identities.insert((release.version.as_str(), release.target.as_str())) {
            return Err(invalid("update feed contains duplicate version and target").into());
        }
    }
    Ok(payload.releases)
}

fn sign(payload: Payload, signer: &SigningKey) -> Result<Vec<u8>> {
    let payload = serde_json::value::to_raw_value(&payload)?;
    let signature = signer.sign(payload.get().as_bytes());
    let envelope = SignedEnvelope {
        version: 1,
        payload,
        signature: STANDARD.encode(signature.to_bytes()),
    };
    let mut bytes = serde_json::to_vec(&envelope)?;
    bytes.push(b'\n');
    if bytes.len() > MAX_FEED {
        return Err(invalid("signed update feed exceeds its limit").into());
    }
    Ok(bytes)
}

impl Release {
    fn validate(&self, repository: &str) -> Result<()> {
        let version = parse_version(&self.version)?;
        if !version.build.is_empty() {
            return Err(invalid("release versions cannot use build metadata").into());
        }
        let (bundle, executable) = layout(&self.target)?;
        let name = format!("Sailry-{}.zip", self.target);
        let tag = format!("v{}", self.version);
        let url = format!("https://github.com/{repository}/releases/download/{tag}/{name}");
        if self.minimum_system != MACOS_MINIMUM
            || self.name != name
            || self.url != url
            || self.bundle != bundle
            || self.executable != executable
            || self.size == 0
            || self.size > MAX_ARCHIVE
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(invalid("invalid release update metadata").into());
        }
        Ok(())
    }
}

fn layout(target: &str) -> Result<(&'static str, &'static str)> {
    match target {
        "aarch64-apple-darwin" | "x86_64-apple-darwin" => {
            Ok(("Sailry.app", "Contents/MacOS/sailry-desktop"))
        }
        _ => Err(invalid("unsupported update target").into()),
    }
}

fn validate_tag(version: &str, tag: &str) -> Result<()> {
    let parsed = parse_version(version)?;
    if !parsed.build.is_empty() || tag != format!("v{version}") {
        return Err(invalid("release tag does not match its version").into());
    }
    Ok(())
}

fn validate_repository(repository: &str) -> Result<()> {
    let mut parts = repository.split('/');
    let valid = |value: &str| {
        !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    };
    if !parts.next().is_some_and(valid)
        || !parts.next().is_some_and(valid)
        || parts.next().is_some()
    {
        return Err(invalid("invalid release repository").into());
    }
    Ok(())
}

fn parse_version(value: &str) -> Result<Version> {
    Version::parse(value).map_err(|_| invalid("invalid release version").into())
}

fn digest(path: &Path) -> Result<(String, u64)> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let size = std::io::copy(&mut file.by_ref().take(MAX_ARCHIVE + 1), &mut digest)?;
    if size == 0 || size > MAX_ARCHIVE {
        return Err(invalid("update archive size is invalid").into());
    }
    Ok((format!("{:x}", digest.finalize()), size))
}

fn order(left: &Release, right: &Release) -> Ordering {
    let left_version = Version::parse(&left.version).expect("validated release version");
    let right_version = Version::parse(&right.version).expect("validated release version");
    left_version
        .cmp_precedence(&right_version)
        .then_with(|| left.target.cmp(&right.target))
}

#[cfg(test)]
pub(crate) fn decode(
    bytes: &[u8],
    repository: &str,
    channel: Channel,
    ring: &[[u8; 32]],
) -> Result<Vec<Release>> {
    parse(bytes, repository, channel, ring)
}

#[cfg(test)]
pub(crate) fn signed(releases: Vec<Release>, signer: &SigningKey) -> Result<Vec<u8>> {
    sign(Payload { releases }, signer)
}
