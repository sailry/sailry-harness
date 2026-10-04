//! Pinned Chrome for Testing assets, installed only when an agent first uses them.
use super::*;
use std::{io::Write, path::Path};

const VERSION: &str = "153.0.8010.52";
const MAX_ARCHIVE: u64 = 512 * 1024 * 1024;

pub(super) struct Bundle {
    pub browser: PathBuf,
    pub driver: PathBuf,
}

pub(super) fn platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("mac-arm64"),
        ("macos", "x86_64") => Some("mac-x64"),
        ("linux", "x86_64") => Some("linux64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        ("windows", "x86_64") => Some("win64"),
        _ => None,
    }
}

fn paths(root: &Path, platform: &str) -> Bundle {
    let chrome = root.join(format!("chrome-{platform}"));
    let browser = if platform.starts_with("mac-") {
        chrome.join("Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing")
    } else if platform.starts_with("win") {
        chrome.join("chrome.exe")
    } else {
        chrome.join("chrome")
    };
    Bundle {
        browser,
        driver: root.join(format!("chromedriver-{platform}")).join(
            if platform.starts_with("win") {
                "chromedriver.exe"
            } else {
                "chromedriver"
            },
        ),
    }
}

pub(super) async fn install(profile: &Path, stop: &CancellationToken) -> Result<Bundle, Fault> {
    let platform =
        platform().ok_or_else(|| unavailable("managed browser is unsupported on this platform"))?;
    let parent = profile.join("tools/chromium");
    tokio::fs::create_dir_all(&parent).await.map_err(io_error)?;
    let root = parent.join(format!("{VERSION}-{platform}"));
    let bundle = paths(&root, platform);
    if root.join("ready").is_file() && bundle.browser.is_file() && bundle.driver.is_file() {
        return Ok(bundle);
    }
    if root.exists() {
        return Err(unavailable("managed browser installation is incomplete"));
    }
    let staging = Arc::new(tempfile::tempdir_in(parent).map_err(io_error)?);
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|_| unavailable("browser download client unavailable"))?;
    for asset in ["chrome", "chromedriver"] {
        let url = format!(
            "https://storage.googleapis.com/chrome-for-testing-public/{VERSION}/{platform}/{asset}-{platform}.zip"
        );
        let download = async {
            let mut response = client
                .get(url)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|_| unavailable("managed browser download failed"))?;
            if response
                .content_length()
                .is_some_and(|size| size > MAX_ARCHIVE)
            {
                return Err(unavailable("browser archive exceeds the download limit"));
            }
            let mut archive = tempfile::tempfile().map_err(io_error)?;
            let mut size = 0;
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| unavailable("browser download interrupted"))?
            {
                size += chunk.len() as u64;
                if size > MAX_ARCHIVE {
                    return Err(unavailable("browser archive exceeds the download limit"));
                }
                // Keep blocking file writes away from the async worker and bound retained data to one chunk.
                archive = tokio::task::spawn_blocking(move || {
                    archive.write_all(&chunk).map_err(io_error)?;
                    Ok::<_, Fault>(archive)
                })
                .await
                .map_err(|_| unavailable("browser archive writer failed"))??;
            }
            let output = staging.clone();
            tokio::task::spawn_blocking(move || {
                let mut archive = zip::ZipArchive::new(archive)
                    .map_err(|_| unavailable("invalid browser archive"))?;
                let mut total = 0u64;
                for index in 0..archive.len() {
                    total = total.saturating_add(
                        archive
                            .by_index(index)
                            .map_err(|_| unavailable("invalid browser archive"))?
                            .size(),
                    );
                    if total > 2 * 1024 * 1024 * 1024 {
                        return Err(unavailable("browser archive exceeds extraction limit"));
                    }
                }
                archive
                    .extract(output.path())
                    .map_err(|_| unavailable("browser archive extraction failed"))
            })
            .await
            .map_err(|_| unavailable("browser archive extraction failed"))??;
            Ok::<_, Fault>(())
        };
        tokio::select! {
            _ = stop.cancelled() => return Err(unavailable("browser installation cancelled")),
            result = download => result?,
        }
    }
    let staged = paths(staging.path(), platform);
    if !staged.browser.is_file() || !staged.driver.is_file() {
        return Err(unavailable("browser archive is missing its executables"));
    }
    std::fs::write(staging.path().join("ready"), VERSION).map_err(io_error)?;
    std::fs::rename(staging.path(), &root).map_err(io_error)?;
    Ok(bundle)
}
