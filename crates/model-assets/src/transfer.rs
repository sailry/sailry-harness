use crate::{Download, Error, File, Phase, Progress, Result, progress, resume, storage, validate};
use reqwest::{Client, StatusCode, header};
use sha2::Digest;
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
        .map_err(Error::download)?;
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
    let mut phase = Phase::Inspecting;
    notify(Progress {
        phase,
        ..progress(completed, total, transferred, started.elapsed())
    });
    for download in downloads {
        let specification = &download.file;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let _lease = resume::lock(path, specification.name, cancel).await?;
        storage::target(&directory, specification.name)?;
        let check = directory.try_clone().map_err(Error::storage)?;
        let name = specification.name.to_owned();
        let checksum = specification.sha256.to_owned();
        let size = specification.size;
        let stopping = cancel.clone();
        phase = Phase::Validating;
        notify(Progress {
            phase,
            ..progress(completed, total, transferred, started.elapsed())
        });
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
        .map_err(Error::storage)?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if cached {
            completed += specification.size;
            notify(Progress {
                phase,
                ..progress(completed, total, transferred, started.elapsed())
            });
            continue;
        }
        let checkpoint = directory.try_clone().map_err(Error::storage)?;
        let name = specification.name.to_owned();
        let checksum = specification.sha256.to_owned();
        let size = specification.size;
        let url = download.url.clone();
        let stopping = cancel.clone();
        let mut part = tokio::task::spawn_blocking(move || {
            resume::Part::open(
                checkpoint,
                &Download {
                    file: File {
                        name: &name,
                        size,
                        sha256: &checksum,
                    },
                    url,
                },
                &stopping,
            )
        })
        .await
        .map_err(Error::storage)??;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if part.received == specification.size
            && format!("{:x}", part.hash.clone().finalize())
                .eq_ignore_ascii_case(specification.sha256)
        {
            phase = Phase::Installing;
            notify(Progress {
                phase,
                ..progress(
                    completed + part.received,
                    total,
                    transferred,
                    started.elapsed(),
                )
            });
            part.publish(specification.name)?;
            completed += specification.size;
            notify(Progress {
                phase,
                ..progress(completed, total, transferred, started.elapsed())
            });
            continue;
        }
        let mut output = tokio::fs::File::from_std(part.file.try_clone().map_err(Error::storage)?);
        phase = Phase::Downloading;
        notify(Progress {
            phase,
            ..progress(
                completed + part.received,
                total,
                transferred,
                started.elapsed(),
            )
        });
        let result = receive(
            download,
            &mut part,
            &mut output,
            client,
            cancel,
            |received, bytes| {
                transferred += bytes;
                notify(progress(
                    completed + received,
                    total,
                    transferred,
                    started.elapsed(),
                ));
            },
        )
        .await;
        output.flush().await.map_err(Error::storage)?;
        output.sync_all().await.map_err(Error::storage)?;
        drop(output);
        if let Err(error) = result {
            if error == Error::InvalidAsset || part.received == 0 {
                part.discard()?;
            } else {
                part.checkpoint()?;
            }
            return Err(error);
        }
        phase = Phase::Validating;
        notify(Progress {
            phase,
            ..progress(
                completed + part.received,
                total,
                transferred,
                started.elapsed(),
            )
        });
        if part.received != specification.size
            || !format!("{:x}", part.hash.clone().finalize())
                .eq_ignore_ascii_case(specification.sha256)
        {
            part.discard()?;
            return Err(Error::InvalidAsset);
        }
        if cancel.is_cancelled() {
            part.checkpoint()?;
            return Err(Error::Cancelled);
        }
        phase = Phase::Installing;
        notify(Progress {
            phase,
            ..progress(
                completed + part.received,
                total,
                transferred,
                started.elapsed(),
            )
        });
        part.publish(specification.name)?;
        completed += specification.size;
    }
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    notify(Progress {
        // Asset completion does not prove the consumer's native model validation.
        phase,
        percent: 100.,
        bytes_per_second: 0.,
    });
    Ok(path.to_owned())
}

async fn receive(
    download: &Download<'_>,
    part: &mut resume::Part,
    output: &mut tokio::fs::File,
    client: &Client,
    cancel: &CancellationToken,
    mut notify: impl FnMut(u64, u64),
) -> Result<()> {
    let size = download.file.size;
    for attempt in 0..2 {
        let mut request = client
            .get(download.url.clone())
            .header(
                header::USER_AGENT,
                concat!("Sailry/", env!("CARGO_PKG_VERSION")),
            )
            .header(header::ACCEPT_ENCODING, "identity");
        if let Some(validator) = part.range() {
            request = request
                .header(header::RANGE, format!("bytes={}-", part.received))
                .header(header::IF_RANGE, validator);
        }
        let mut response = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(Error::Cancelled),
            response = request.send() => response.map_err(Error::download)?,
        };
        match response.status() {
            StatusCode::PARTIAL_CONTENT
                if part.range().is_some()
                    && resume::range(&response, part.received, size)
                    && part.matches(&response) => {}
            StatusCode::OK => {
                part.reset()?;
                if response
                    .content_length()
                    .is_some_and(|length| length != size)
                {
                    return Err(Error::InvalidAsset);
                }
            }
            StatusCode::PARTIAL_CONTENT | StatusCode::RANGE_NOT_SATISFIABLE => {
                if attempt != 0 || part.range().is_none() {
                    return Err(Error::InvalidAsset);
                }
                if response.status() == StatusCode::RANGE_NOT_SATISFIABLE
                    && response
                        .headers()
                        .get(header::CONTENT_RANGE)
                        .and_then(|value| value.to_str().ok())
                        != Some(format!("bytes */{size}").as_str())
                {
                    return Err(Error::InvalidAsset);
                }
                part.reset()?;
                continue;
            }
            _ => {
                return Err(Error::Download(format!(
                    "HTTP {} for {}",
                    response.status(),
                    download.file.name
                )));
            }
        }
        if response
            .headers()
            .get(header::CONTENT_ENCODING)
            .is_some_and(|value| value != "identity")
        {
            return Err(Error::InvalidAsset);
        }
        part.headers(&response);
        part.checkpoint()?;
        notify(part.received, 0);
        let mut checkpoint = part.received;
        loop {
            let chunk = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(Error::Cancelled),
                chunk = response.chunk() => chunk.map_err(Error::download)?,
            };
            let Some(chunk) = chunk else {
                return Ok(());
            };
            let received = part
                .received
                .checked_add(chunk.len() as u64)
                .filter(|received| *received <= size)
                .ok_or(Error::InvalidAsset)?;
            // Finish each bounded body write before recording its prefix hash.
            // Dropping write_all on cancellation could leave a half-written chunk.
            output.write_all(&chunk).await.map_err(Error::storage)?;
            part.received = received;
            part.hash.update(&chunk);
            if received - checkpoint >= 8 * 1024 * 1024 {
                output.flush().await.map_err(Error::storage)?;
                part.checkpoint()?;
                checkpoint = received;
            }
            notify(received, chunk.len() as u64);
        }
    }
    Err(Error::InvalidAsset)
}
