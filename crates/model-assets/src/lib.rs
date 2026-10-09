//! Pinned model-file acquisition extracted from Sailry's speech asset downloader.
#[cfg(feature = "onnx")]
pub mod onnx;
mod resume;
pub mod runtime;
mod storage;
mod transfer;
mod urls;

use std::{collections::BTreeSet, path::Path, time::Duration};
pub use transfer::prepare;
pub use urls::url;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("model asset storage failed")]
    Storage,
    #[error("model asset download failed")]
    Download,
    #[error("model asset validation failed")]
    InvalidAsset,
    #[error("model asset download cancelled")]
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct File<'a> {
    pub name: &'a str,
    pub size: u64,
    pub sha256: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Download<'a> {
    pub file: File<'a>,
    pub url: reqwest::Url,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Progress {
    pub percent: f32,
    pub bytes_per_second: f64,
}

fn progress(completed: u64, total: u64, transferred: u64, elapsed: Duration) -> Progress {
    Progress {
        percent: (completed as f32 / total.max(1) as f32 * 100.).min(99.),
        // Cached bytes count toward completion, never toward network throughput.
        bytes_per_second: transferred as f64 / elapsed.as_secs_f64().max(0.001),
    }
}

pub fn ready(directory: &Path, files: &[File<'_>]) -> bool {
    if validate(files).is_err() {
        return false;
    }
    let Ok(directory) = storage::open(directory, false) else {
        return false;
    };
    files
        .iter()
        .all(|file| storage::valid(&directory, file, None))
}

fn validate(files: &[File<'_>]) -> Result<u64> {
    if files.is_empty() {
        return Err(Error::InvalidAsset);
    }
    let mut names = BTreeSet::new();
    let mut total = 0u64;
    for file in files {
        let stem = file
            .name
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        if file.name.is_empty()
            || file.name.len() > 128
            || file.name.starts_with('.')
            || file.name.ends_with('.')
            || !file
                .name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ["COM", "LPT"].iter().any(|prefix| {
                stem.strip_prefix(prefix).is_some_and(|suffix| {
                    matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
                })
            })
            || file.sha256.len() != 64
            || !file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !names.insert(file.name.to_ascii_lowercase())
        {
            return Err(Error::InvalidAsset);
        }
        total = total.checked_add(file.size).ok_or(Error::InvalidAsset)?;
    }
    Ok(total)
}

#[cfg(test)]
mod tests;
