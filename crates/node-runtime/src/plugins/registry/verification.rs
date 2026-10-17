//! Verify the exact UTF-8 payload before interpreting its archive claims.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Read, Seek},
};

#[derive(Deserialize)]
pub(super) struct Envelope {
    signed: bool,
    key_id: String,
    payload: String,
    signature: String,
    content_hash: String,
    hash_version: u32,
}
#[derive(Deserialize)]
pub(super) struct Keys {
    keys: Vec<Key>,
}
#[derive(Deserialize)]
struct Key {
    key_id: String,
    algorithm: String,
    public_key_raw_b64: String,
    status: String,
    issuer: String,
}
#[derive(Deserialize)]
pub(super) struct Payload {
    v: u32,
    issuer: String,
    skill_slug: String,
    skill_version: String,
    pub(super) version_id: u64,
    publisher_id: u64,
    pub(super) content_hash: String,
    pub(super) package_md5: String,
    file_count: usize,
}

fn hex(value: &str, bytes: usize) -> bool {
    value.len() == bytes * 2
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

pub(super) fn signature(
    envelope: &Envelope,
    keys: &Keys,
    slug: &str,
    version: &str,
) -> Result<Payload, Fault> {
    if !envelope.signed || envelope.hash_version != 1 || envelope.payload.len() > 64 * 1024 {
        return Err(invalid());
    }
    let mut matches = keys.keys.iter().filter(|key| key.key_id == envelope.key_id);
    let key = matches.next().ok_or_else(invalid)?;
    if matches.next().is_some()
        || key.algorithm != "Ed25519"
        || key.status != "active"
        || key.issuer != "skillhub.cn"
    {
        return Err(invalid());
    }
    let bytes: [u8; 32] = STANDARD
        .decode(&key.public_key_raw_b64)
        .map_err(|_| invalid())?
        .try_into()
        .map_err(|_| invalid())?;
    let signature = STANDARD
        .decode(&envelope.signature)
        .map_err(|_| invalid())?;
    let signature = ed25519_dalek::Signature::from_slice(&signature).map_err(|_| invalid())?;
    ed25519_dalek::VerifyingKey::from_bytes(&bytes)
        .and_then(|key| key.verify_strict(envelope.payload.as_bytes(), &signature))
        .map_err(|_| invalid())?;
    let payload: Payload = serde_json::from_str(&envelope.payload).map_err(|_| invalid())?;
    if payload.v != 1
        || payload.issuer != key.issuer
        || payload.skill_slug != slug
        || payload.skill_version != version
        || payload.version_id == 0
        || payload.publisher_id == 0
        || payload.file_count == 0
        || payload.file_count > 10_000
        || !hex(&payload.content_hash, 32)
        || !hex(&payload.package_md5, 16)
        || envelope.content_hash != payload.content_hash
    {
        return Err(invalid());
    }
    Ok(payload)
}

fn excluded(path: &str) -> bool {
    path.split('/').any(|part| {
        part == "__MACOSX"
            || part == ".DS_Store"
            || part.eq_ignore_ascii_case("Thumbs.db")
            || part.starts_with("._")
    })
}

fn content_hash(entries: &BTreeMap<String, String>, include_meta: bool) -> (String, usize) {
    let mut content = Sha256::new();
    let mut count = 0;
    for (path, digest) in entries {
        if !include_meta && path == "_meta.json" {
            continue;
        }
        content.update(format!("{path}:{digest}\n").as_bytes());
        count += 1;
    }
    (format!("{:x}", content.finalize()), count)
}

pub(super) fn archive(
    mut file: File,
    mut resolved: RegistryResolved,
    payload: &Payload,
    stop: &CancellationToken,
) -> Result<(File, RegistryResolved), Fault> {
    file.rewind().map_err(io_error)?;
    let mut sha = Sha256::new();
    let mut md5 = md5::Md5::new();
    let mut buffer = [0u8; 32 * 1024];
    loop {
        if stop.is_cancelled() {
            return Err(super::super::github::cancelled());
        }
        let count = file.read(&mut buffer).map_err(io_error)?;
        if count == 0 {
            break;
        }
        sha.update(&buffer[..count]);
        md5.update(&buffer[..count]);
    }
    if format!("{:x}", md5.finalize()) != payload.package_md5 {
        return Err(invalid());
    }
    resolved.archive_sha256 = format!("{:x}", sha.finalize());
    file.rewind().map_err(io_error)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| invalid())?;
    if zip.len() > 10_000 {
        return Err(invalid());
    }
    let mut names = std::collections::BTreeSet::new();
    let mut entries = BTreeMap::new();
    let mut expanded = 0u64;
    for index in 0..zip.len() {
        if stop.is_cancelled() {
            return Err(super::super::github::cancelled());
        }
        let mut entry = zip.by_index(index).map_err(|_| invalid())?;
        let path = entry.name().to_owned();
        let relative = path.strip_suffix('/').unwrap_or(&path);
        // Do not normalize attacker-controlled ZIP names into a different signed tree.
        if relative.is_empty()
            || relative.contains('\\')
            || relative.starts_with('/')
            || relative
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || entry.enclosed_name().is_none()
            || entry.encrypted()
            || !names.insert(relative.to_owned())
        {
            return Err(invalid());
        }
        if entry.is_dir() {
            continue;
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| !matches!(mode & 0o170000, 0 | 0o100000))
        {
            return Err(invalid());
        }
        expanded = expanded
            .checked_add(entry.size())
            .filter(|size| *size <= sailry_protocol::plugin::MAX_PACKAGE_BYTES)
            .ok_or_else(invalid)?;
        let mut digest = Sha256::new();
        let mut actual = 0u64;
        loop {
            let count = entry.read(&mut buffer).map_err(io_error)?;
            if count == 0 {
                break;
            }
            actual = actual
                .checked_add(count as u64)
                .filter(|size| *size <= entry.size())
                .ok_or_else(invalid)?;
            digest.update(&buffer[..count]);
        }
        if actual != entry.size() {
            return Err(invalid());
        }
        if !excluded(&path) {
            entries.insert(path, format!("{:x}", digest.finalize()));
        }
    }
    // Skillhub hash v1 signs either the content-only tree or its historical
    // metadata-inclusive form. Both must match the same authenticated claim.
    if ![false, true].into_iter().any(|include_meta| {
        let (hash, count) = content_hash(&entries, include_meta);
        hash == payload.content_hash && count == payload.file_count
    }) {
        return Err(invalid());
    }
    let mut file = zip.into_inner();
    file.rewind().map_err(io_error)?;
    Ok((file, resolved))
}
