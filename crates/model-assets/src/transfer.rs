use crate::{Download, Error, File, Progress, Result, progress, storage, validate};
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

pub async fn prepare(
    directory: &Path,
    downloads: &[Download<'_>],
    cancel: &CancellationToken,
    progress: impl FnMut(Progress),
) -> Result<PathBuf> {
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    for download in downloads {
        let url = &download.url;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::InvalidAsset);
        }
    }
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(|_| Error::Download)?;
    acquire(directory, downloads, cancel, progress, &client).await
}

pub(super) async fn acquire(
    path: &Path,
    downloads: &[Download<'_>],
    cancel: &CancellationToken,
    mut notify: impl FnMut(Progress),
    client: &Client,
) -> Result<PathBuf> {
    let files: Vec<_> = downloads.iter().map(|download| download.file).collect();
    let total = validate(&files)?;
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let directory = storage::open(path, true)?;
    let started = Instant::now();
    let mut completed = 0u64;
    let mut transferred = 0u64;
    for download in downloads {
        let specification = &download.file;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        storage::target(&directory, specification.name)?;
        let check = directory.try_clone().map_err(|_| Error::Storage)?;
        let name = specification.name.to_owned();
        let checksum = specification.sha256.to_owned();
        let size = specification.size;
        let stopping = cancel.clone();
        let cached = tokio::task::spawn_blocking(move || {
            storage::valid(
                &check,
                &File {
                    name: &name,
                    size,
                    sha256: &checksum,
                },
                Some(&stopping),
            )
        })
        .await
        .map_err(|_| Error::Storage)?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if cached {
            completed += specification.size;
            notify(progress(completed, total, transferred, started.elapsed()));
            continue;
        }
        let temporary = storage::temporary(&directory)?;
        let mut output = tokio::fs::File::from_std(
            temporary
                .as_file()
                .try_clone()
                .map_err(|_| Error::Storage)?
                .into_std(),
        );
        let mut response = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(Error::Cancelled),
            response = client.get(download.url.clone()).send() => response.and_then(reqwest::Response::error_for_status).map_err(|_| Error::Download)?,
        };
        if response
            .content_length()
            .is_some_and(|size| size != specification.size)
        {
            return Err(Error::InvalidAsset);
        }
        let mut received = 0u64;
        let mut hash = Sha256::new();
        loop {
            let chunk = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(Error::Cancelled),
                chunk = response.chunk() => chunk.map_err(|_| Error::Download)?,
            };
            let Some(chunk) = chunk else {
                break;
            };
            received = received
                .checked_add(chunk.len() as u64)
                .filter(|size| *size <= specification.size)
                .ok_or(Error::InvalidAsset)?;
            transferred = transferred
                .checked_add(chunk.len() as u64)
                .ok_or(Error::InvalidAsset)?;
            hash.update(&chunk);
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(Error::Cancelled),
                written = output.write_all(&chunk) => written.map_err(|_| Error::Storage)?,
            }
            notify(progress(
                completed + received,
                total,
                transferred,
                started.elapsed(),
            ));
        }
        if received != specification.size
            || !format!("{:x}", hash.finalize()).eq_ignore_ascii_case(specification.sha256)
        {
            return Err(Error::InvalidAsset);
        }
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(Error::Cancelled),
            synced = async { output.flush().await?; output.sync_all().await } => synced.map_err(|_| Error::Storage)?,
        }
        drop(output);
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        storage::target(&directory, specification.name)?;
        temporary
            .replace(specification.name)
            .map_err(|_| Error::Storage)?;
        completed += specification.size;
    }
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    notify(Progress {
        percent: 100.,
        bytes_per_second: 0.,
    });
    Ok(path.to_owned())
}
