//! Fixed storage adapters keep segmented objects bounded on WebDAV and S3.
use super::{MAX_BYTES, MAX_MANIFEST, Plan, archive, cancelled, io};
use futures::TryStreamExt;
use opendal::{Operator, layers::HttpClientLayer, raw::HttpClient, services};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault,
    backup::{Kind, Page, Record},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path, time::Duration};
use tokio::io::AsyncWriteExt;

const PART_BYTES: usize = 4 * 1024 * 1024;
const MAX_RECORDS: usize = 1000;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Part {
    size: u32,
    sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    record: Record,
    parts: Vec<Part>,
}

impl Plan {
    fn operator(&self) -> Result<Operator, Fault> {
        self.destination.validate()?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(60))
            .retry(reqwest::retry::never())
            .build()
            .map_err(io)?;
        let client = HttpClientLayer::new(HttpClient::with(client));
        let root = format!("/{}/", self.destination.root.trim_matches('/'));
        let identity = self
            .auth
            .identity
            .as_ref()
            .map_or("", |value| value.expose());
        let secret = self.auth.secret.as_ref().map_or("", |value| value.expose());
        let operator = match self.destination.kind {
            Kind::Webdav => Operator::new(
                services::Webdav::default()
                    .endpoint(&self.destination.endpoint)
                    .root(&root)
                    .username(identity)
                    .password(secret),
            )
            .map_err(storage)?
            .layer(client)
            .finish(),
            Kind::S3 => {
                if identity.is_empty() || secret.is_empty() {
                    return Err(Fault::new(
                        ErrorCode::NotConfigured,
                        "S3 credentials are missing",
                    ));
                }
                let mut builder = services::S3::default()
                    .endpoint(&self.destination.endpoint)
                    .root(&root)
                    .bucket(&self.destination.bucket)
                    .region(&self.destination.region)
                    .disable_config_load()
                    .disable_ec2_metadata()
                    .access_key_id(identity)
                    .secret_access_key(secret);
                if let Some(token) = &self.auth.session_token {
                    builder = builder.session_token(token.expose());
                }
                Operator::new(builder)
                    .map_err(storage)?
                    .layer(client)
                    .finish()
            }
        };
        Ok(operator)
    }
}

impl Manifest {
    fn validate(&self, id: &str) -> Result<(), Fault> {
        self.record.validate()?;
        if self.version != 1
            || self.record.id != id
            || self.record.size_bytes > MAX_BYTES
            || self.parts.is_empty()
            || self.parts.len() > (MAX_BYTES / PART_BYTES as u64) as usize + 1
        {
            return Err(Fault::invalid("invalid remote backup manifest"));
        }
        let mut total = 0u64;
        for (index, part) in self.parts.iter().enumerate() {
            if part.size == 0
                || part.size as usize > PART_BYTES
                || (index + 1 < self.parts.len() && part.size as usize != PART_BYTES)
                || !archive::digest(&part.sha256)
            {
                return Err(Fault::invalid("invalid backup segment metadata"));
            }
            total = total
                .checked_add(u64::from(part.size))
                .ok_or_else(|| Fault::invalid("backup size overflow"))?;
        }
        if total != self.record.size_bytes {
            return Err(Fault::invalid("backup segment sizes differ from total"));
        }
        Ok(())
    }
}

pub(crate) async fn export(
    plan: &Plan,
    packed: archive::Packed,
    id: String,
    cancel: &CancellationToken,
) -> Result<Record, Fault> {
    cancelled(cancel)?;
    let passphrase = super::encryption::passphrase(plan.auth.passphrase.as_ref())?.clone();
    let stop = cancel.clone();
    let packed =
        tokio::task::spawn_blocking(move || super::encryption::seal(packed, &passphrase, &stop))
            .await
            .map_err(io)??;
    let op = plan.operator()?;
    // A request-derived directory is owned by this one export; no shared object is replaced.
    let directory = format!("{id}/");
    step(cancel, op.create_dir(&directory)).await?;
    let mut input = packed.file.reopen().map_err(io)?;
    let mut parts = Vec::new();
    let mut buffer = vec![0u8; PART_BYTES];
    loop {
        if let Err(error) = cancelled(cancel) {
            cleanup(&op, &id, parts.len()).await;
            return Err(error);
        }
        let mut length = 0;
        while length < buffer.len() {
            let count = input.read(&mut buffer[length..]).map_err(io)?;
            if count == 0 {
                break;
            }
            length += count;
        }
        if length == 0 {
            break;
        }
        let path = part_path(&id, parts.len());
        let result = step(cancel, op.write(&path, buffer[..length].to_vec())).await;
        match result {
            Ok(_) => parts.push(Part {
                size: length as u32,
                sha256: format!("{:x}", Sha256::digest(&buffer[..length])),
            }),
            Err(error) if error.code == ErrorCode::Cancelled => {
                cleanup(&op, &id, parts.len() + 1).await;
                return Err(error);
            }
            Err(error) => {
                cleanup(&op, &id, parts.len() + 1).await;
                return Err(uncertain(error));
            }
        }
    }
    let record = Record {
        id: id.clone(),
        node: packed.snapshot.manifest.node,
        created_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io)?
            .as_millis()
            .min(u64::MAX as u128) as u64,
        size_bytes: packed.size,
        files: packed.snapshot.manifest.files.len() as u32,
        sha256: packed.sha256,
    };
    let manifest = Manifest {
        version: 1,
        record: record.clone(),
        parts,
    };
    manifest.validate(&id)?;
    let bytes = serde_json::to_vec(&manifest).map_err(io)?;
    if bytes.len() > MAX_MANIFEST {
        cleanup(&op, &id, manifest.parts.len()).await;
        return Err(Fault::invalid("backup manifest exceeds the limit"));
    }
    // The manifest is the commit marker. An uncertain write is never repeated or deleted.
    step(cancel, op.write(&format!("{id}.json"), bytes))
        .await
        .map_err(uncertain)?;
    Ok(record)
}

pub(crate) async fn list(plan: &Plan, cancel: &CancellationToken) -> Result<Page, Fault> {
    let op = plan.operator()?;
    let mut entries = step(cancel, op.lister("/")).await?;
    let mut records = Vec::new();
    let mut partial = false;
    let mut inspected = 0;
    loop {
        let entry = tokio::select! { _ = cancel.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "backup listing cancelled")), entry = entries.try_next() => entry.map_err(storage)? };
        let Some(entry) = entry else { break };
        inspected += 1;
        if inspected > MAX_RECORDS * 4 || records.len() >= MAX_RECORDS {
            partial = true;
            break;
        }
        if !entry.metadata().is_file() {
            continue;
        }
        let Some(id) = entry.path().strip_suffix(".json") else {
            continue;
        };
        if uuid::Uuid::parse_str(id).is_err() {
            continue;
        }
        let manifest = read_manifest(&op, id, cancel).await?;
        records.push(manifest.record);
    }
    records.sort_by(|left, right| {
        right
            .created_ms
            .cmp(&left.created_ms)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(Page { records, partial })
}

pub(crate) async fn restore(
    plan: &Plan,
    id: &str,
    temporary: &Path,
    cancel: &CancellationToken,
) -> Result<(Record, tempfile::NamedTempFile), Fault> {
    let op = plan.operator()?;
    let manifest = read_manifest(&op, id, cancel).await?;
    let file = tempfile::NamedTempFile::new_in(temporary).map_err(io)?;
    let mut output = tokio::fs::File::from_std(file.reopen().map_err(io)?);
    let mut digest = Sha256::new();
    for (index, spec) in manifest.parts.iter().enumerate() {
        let path = part_path(id, index);
        let metadata = step(cancel, op.stat(&path)).await?;
        if !metadata.is_file() || metadata.content_length() != u64::from(spec.size) {
            return Err(Fault::invalid("backup segment size changed"));
        }
        let reader = step(cancel, op.reader(&path)).await?;
        let mut stream = step(cancel, reader.into_bytes_stream(..)).await?;
        let mut size = 0u64;
        let mut part_digest = Sha256::new();
        loop {
            let bytes = tokio::select! { _ = cancel.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "backup restore cancelled")), bytes = stream.try_next() => bytes.map_err(io)? };
            let Some(bytes) = bytes else { break };
            size += bytes.len() as u64;
            if size > u64::from(spec.size) {
                return Err(Fault::invalid("backup segment exceeds its declared size"));
            }
            part_digest.update(&bytes);
            digest.update(&bytes);
            output.write_all(&bytes).await.map_err(io)?;
        }
        if size != u64::from(spec.size) || format!("{:x}", part_digest.finalize()) != spec.sha256 {
            return Err(Fault::invalid("backup segment checksum changed"));
        }
    }
    if format!("{:x}", digest.finalize()) != manifest.record.sha256 {
        return Err(Fault::invalid("backup archive checksum changed"));
    }
    output.sync_all().await.map_err(io)?;
    Ok((manifest.record, file))
}

async fn read_manifest(
    op: &Operator,
    id: &str,
    cancel: &CancellationToken,
) -> Result<Manifest, Fault> {
    if !uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id) {
        return Err(Fault::invalid("invalid backup identifier"));
    }
    let path = format!("{id}.json");
    let metadata = step(cancel, op.stat(&path)).await?;
    if !metadata.is_file() || metadata.content_length() > MAX_MANIFEST as u64 {
        return Err(Fault::invalid("backup manifest exceeds the limit"));
    }
    let reader = step(cancel, op.reader(&path)).await?;
    let mut stream = step(cancel, reader.into_bytes_stream(..)).await?;
    let mut bytes = Vec::new();
    loop {
        let chunk = tokio::select! { _ = cancel.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "backup read cancelled")), chunk = stream.try_next() => chunk.map_err(io)? };
        let Some(chunk) = chunk else { break };
        if bytes.len().saturating_add(chunk.len()) > MAX_MANIFEST {
            return Err(Fault::invalid("backup manifest exceeds the limit"));
        }
        bytes.extend_from_slice(&chunk);
    }
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|_| Fault::invalid("invalid remote backup manifest"))?;
    manifest.validate(id)?;
    Ok(manifest)
}

async fn step<T>(
    cancel: &CancellationToken,
    future: impl std::future::Future<Output = opendal::Result<T>>,
) -> Result<T, Fault> {
    tokio::select! { biased; _ = cancel.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "profile backup cancelled")), result = future => result.map_err(storage) }
}

async fn cleanup(op: &Operator, id: &str, parts: usize) {
    let _ = tokio::time::timeout(Duration::from_secs(10), async {
        for index in 0..parts {
            let _ = op.delete(&part_path(id, index)).await;
        }
        let _ = op.delete(&format!("{id}/")).await;
    })
    .await;
}

fn part_path(id: &str, index: usize) -> String {
    format!("{id}/{index:06}.bin")
}

fn storage(error: opendal::Error) -> Fault {
    let code = match error.kind() {
        opendal::ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
        opendal::ErrorKind::NotFound => ErrorCode::NotFound,
        _ => ErrorCode::Unavailable,
    };
    // OpenDAL diagnostics may contain signed URLs; keep them inside the private transport.
    Fault::new(code, "backup storage request failed")
}

fn uncertain(error: Fault) -> Fault {
    if matches!(
        error.code,
        ErrorCode::PermissionDenied | ErrorCode::InvalidRequest
    ) {
        error
    } else {
        Fault::new(
            ErrorCode::OutcomeUnknown,
            "backup export outcome is unknown; inspect the remote backup list",
        )
    }
}
