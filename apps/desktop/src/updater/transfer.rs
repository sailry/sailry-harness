//! Network work stages a verified file separately from installation.
use super::{
    Failure, Result,
    config::{self, Config, MAX_MANIFEST},
    manifest::{self, Selection},
    package,
};
use sailry_link::CancellationToken;
use std::{
    fs,
    io::Write as _,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Progress {
    Downloading { copied: u64, total: Option<u64> },
    Verifying,
}

pub(super) struct Staged {
    pub directory: tempfile::TempDir,
    pub archive: PathBuf,
    pub selection: Selection,
}

pub(super) fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .no_proxy()
        .referer(false)
        .redirect(reqwest::redirect::Policy::custom(|attempt| match redirect(
            attempt.url(),
            attempt.previous(),
        ) {
            Ok(()) => attempt.follow(),
            Err(error) => attempt.error(error),
        }))
        .retry(reqwest::retry::never())
        .build()
        .map_err(|_| Failure::new("updates_network_failed", "the update HTTP client failed"))
}

pub(super) fn redirect(next: &url::Url, previous: &[url::Url]) -> Result<()> {
    // reqwest supplies the normalized destination; URI fragments never go on the wire.
    config::validate_url(next.as_str())?;
    if previous
        .last()
        .is_some_and(|url| url.scheme() == "https" && next.scheme() != "https")
    {
        return Err(Failure::new(
            "updates_network_failed",
            "update redirects must not downgrade HTTPS",
        ));
    }
    // The first previous URL is the initial request, not a redirect hop.
    if previous.len() > 5 || previous.contains(next) {
        return Err(Failure::new(
            "updates_network_failed",
            "update redirects exceed their limit or repeat a requested URL",
        ));
    }
    Ok(())
}

pub(super) async fn check(
    client: reqwest::Client,
    config: &Config,
    stop: &CancellationToken,
) -> Result<Option<Selection>> {
    let request = async {
        let mut response = client
            .get(&config.source)
            .timeout(Duration::from_secs(45))
            .send()
            .await
            .map_err(|_| {
                Failure::new(
                    "updates_network_failed",
                    "the update metadata request failed",
                )
            })?;
        if !response.status().is_success() {
            return Err(Failure::new(
                "updates_source_unready",
                format!(
                    "the update metadata server returned HTTP {}",
                    response.status().as_u16()
                ),
            ));
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_MANIFEST as u64)
        {
            return Err(Failure::new(
                "updates_manifest_invalid",
                "update metadata exceeds its size limit",
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| {
            Failure::new(
                "updates_network_failed",
                "the update metadata transfer was interrupted",
            )
        })? {
            if bytes.len().saturating_add(chunk.len()) > MAX_MANIFEST {
                return Err(Failure::new(
                    "updates_manifest_invalid",
                    "update metadata exceeds its size limit",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        manifest::select(&bytes, config)
    };
    tokio::select! {
        biased;
        _ = stop.cancelled() => Err(cancelled()),
        result = request => result,
    }
}

pub(super) async fn download(
    client: reqwest::Client,
    config: Config,
    selection: Selection,
    root: &Path,
    stop: CancellationToken,
    progress: Arc<dyn Fn(Progress) + Send + Sync>,
) -> Result<Staged> {
    manifest::revalidate(&selection, &config)?;
    fs::create_dir_all(root).map_err(Failure::io)?;
    let directory = tempfile::Builder::new()
        .prefix("download-")
        .tempdir_in(root)
        .map_err(Failure::io)?;
    let archive = directory.path().join(&selection.release.name);
    let mut destination = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&archive)
        .map_err(Failure::io)?;
    // Stage through the shared client so cancellation cannot install early.
    let download = async {
        let mut response = client
            .get(&selection.release.url)
            .timeout(Duration::from_secs(30 * 60))
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .send()
            .await
            .map_err(|_| {
                Failure::new("updates_network_failed", "the update asset request failed")
            })?;
        if !response.status().is_success() {
            return Err(Failure::new(
                "updates_network_failed",
                "the update asset server rejected the request",
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length != selection.release.size)
        {
            return Err(Failure::new(
                "updates_package_invalid",
                "the update asset length differs from signed metadata",
            ));
        }
        let mut copied = 0_u64;
        let mut last = Instant::now();
        while let Some(chunk) = response.chunk().await.map_err(|_| {
            Failure::new(
                "updates_network_failed",
                "the update asset transfer was interrupted",
            )
        })? {
            copied = copied
                .checked_add(chunk.len() as u64)
                .filter(|size| *size <= selection.release.size)
                .ok_or_else(|| {
                    Failure::new(
                        "updates_package_invalid",
                        "the update asset exceeds its signed size",
                    )
                })?;
            // This task runs on the separate process executor, not the GPUI thread.
            // Complete each small write before cancellation drops its staging owner.
            destination.write_all(&chunk).map_err(Failure::io)?;
            if copied == selection.release.size || last.elapsed() >= Duration::from_millis(75) {
                progress(Progress::Downloading {
                    copied,
                    total: Some(selection.release.size),
                });
                last = Instant::now();
            }
        }
        if copied != selection.release.size {
            return Err(Failure::new(
                "updates_package_invalid",
                "the update asset is incomplete",
            ));
        }
        Ok(())
    };
    progress(Progress::Downloading {
        copied: 0,
        total: Some(selection.release.size),
    });
    tokio::select! {
        biased;
        _ = stop.cancelled() => return Err(cancelled()),
        result = download => result?,
    }
    destination.sync_all().map_err(Failure::io)?;
    drop(destination);
    if stop.is_cancelled() {
        return Err(cancelled());
    }
    progress(Progress::Verifying);
    let staged = Staged {
        directory,
        archive,
        selection,
    };
    // Cancellation remains available while verifying, but waits for this read-only tail to end.
    // Dropping a running blocking task would not stop it, so never detach it from staging ownership.
    let staged = tokio::task::spawn_blocking(move || {
        package::verify_archive(&staged.archive, &staged.selection.release, &config.keys)?;
        let destination = staged.directory.path().join("contents");
        fs::create_dir(&destination).map_err(Failure::io)?;
        package::extract(&staged.archive, &staged.selection.release, &destination)?;
        Ok::<_, Failure>(staged)
    })
    .await
    .map_err(|_| Failure::new("updates_package_invalid", "update verification task failed"))??;
    if stop.is_cancelled() {
        return Err(cancelled());
    }
    Ok(staged)
}

pub(super) fn cancelled() -> Failure {
    Failure::new("updates_cancelled", "update transfer was cancelled")
}
