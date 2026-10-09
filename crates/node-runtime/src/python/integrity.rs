use super::{check, storage};
use sailry_link::CancellationToken;
use sailry_protocol::Fault;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    time::SystemTime,
};

const MARKER: &str = ".sailry-ready.json";
const MAX_FILES: usize = 100_000;
const MAX_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Default)]
pub(super) struct Cache(BTreeMap<PathBuf, (Stamp, String)>);

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

impl Stamp {
    fn new(metadata: &std::fs::Metadata) -> Self {
        Self {
            size: metadata.len(),
            modified: metadata.modified().ok(),
            created: metadata.created().ok(),
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                )
            },
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct File {
    pub path: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    key: String,
    files: Vec<File>,
}

pub(super) fn key(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(super) fn seal(
    root: &Path,
    runtime: &Path,
    key: &str,
    cancel: &CancellationToken,
) -> Result<(), Fault> {
    let paths = paths(root, cancel)?;
    let mut files = Vec::with_capacity(paths.len());
    let mut total = 0u64;
    for path in paths {
        check(cancel)?;
        let absolute = root.join(&path);
        let metadata = absolute.symlink_metadata().map_err(storage)?;
        let file = if metadata.is_symlink() {
            let target = absolute.canonicalize().map_err(storage)?;
            if !target.starts_with(root) && !target.starts_with(runtime) {
                return Err(Fault::invalid(
                    "Python environment link points outside its runtime",
                ));
            }
            File {
                path,
                size: 0,
                sha256: String::new(),
                link: Some(
                    std::fs::read_link(absolute)
                        .map_err(storage)?
                        .into_os_string()
                        .into_string()
                        .map_err(|_| Fault::invalid("Python link is not Unicode"))?,
                ),
            }
        } else {
            total = total
                .checked_add(metadata.len())
                .filter(|total| *total <= MAX_BYTES)
                .ok_or_else(|| Fault::invalid("Python environment exceeds its size limit"))?;
            File {
                path,
                size: metadata.len(),
                sha256: hash(&absolute, cancel)?,
                link: None,
            }
        };
        files.push(file);
    }
    let receipt = Receipt {
        version: 1,
        key: key.into(),
        files,
    };
    std::fs::write(
        root.join(MARKER),
        serde_json::to_vec(&receipt).map_err(storage)?,
    )
    .map_err(storage)
}

pub(super) fn valid(
    root: &Path,
    runtime: &Path,
    expected: &str,
    cache: &mut Cache,
    cancel: &CancellationToken,
) -> Result<bool, Fault> {
    check(cancel)?;
    let result = (|| {
        let metadata = root.symlink_metadata().map_err(storage)?;
        if !metadata.is_dir() || metadata.is_symlink() {
            return Ok(false);
        }
        let metadata = root.join(MARKER).symlink_metadata().map_err(storage)?;
        if !metadata.is_file() || metadata.is_symlink() || metadata.len() > 16 * 1024 * 1024 {
            return Ok(false);
        }
        let receipt: Receipt =
            serde_json::from_slice(&std::fs::read(root.join(MARKER)).map_err(storage)?)
                .map_err(storage)?;
        if receipt.version != 1 || receipt.key != expected || !key(expected) {
            return Ok(false);
        }
        let actual = paths(root, cancel)?;
        let listed = receipt
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>();
        if actual != listed {
            return Ok(false);
        }
        verify(root, runtime, &receipt.files, cache, cancel)
    })();
    match result {
        Err(error) if error.code != sailry_protocol::ErrorCode::Cancelled => Ok(false),
        result => result,
    }
}

pub(super) fn verify(
    root: &Path,
    runtime: &Path,
    files: &[File],
    cache: &mut Cache,
    cancel: &CancellationToken,
) -> Result<bool, Fault> {
    let mut total = 0u64;
    let mut seen = std::collections::BTreeSet::new();
    if files.is_empty() || files.len() > MAX_FILES {
        return Ok(false);
    }
    for file in files {
        check(cancel)?;
        if file.path.is_empty()
            || file.path.contains('\\')
            || file
                .path
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
            || !seen.insert(&file.path)
        {
            return Ok(false);
        }
        let path = root.join(&file.path);
        let metadata = path.symlink_metadata().map_err(storage)?;
        let resolved = path.canonicalize().map_err(storage)?;
        if !resolved.starts_with(root) && !resolved.starts_with(runtime) {
            return Ok(false);
        }
        if let Some(link) = &file.link {
            if !metadata.is_symlink()
                || std::fs::read_link(&path).map_err(storage)?.as_os_str()
                    != std::ffi::OsStr::new(link)
            {
                return Ok(false);
            }
            continue;
        }
        // Bundled receipt producers may list interpreter aliases as regular data.
        // Such links must still resolve within the bundled runtime tree.
        let metadata = if metadata.is_symlink() {
            path.metadata().map_err(storage)?
        } else {
            metadata
        };
        if !metadata.is_file() || metadata.len() != file.size || !key(&file.sha256) {
            return Ok(false);
        }
        total = total
            .checked_add(file.size)
            .filter(|total| *total <= MAX_BYTES)
            .ok_or_else(|| Fault::invalid("Python environment exceeds its size limit"))?;
        let stamp = Stamp::new(&metadata);
        let digest = match cache.0.get(&path) {
            Some((previous, digest)) if previous == &stamp => digest.clone(),
            _ => {
                let digest = hash(&path, cancel)?;
                cache.0.insert(path.clone(), (stamp, digest.clone()));
                digest
            }
        };
        if digest != file.sha256 {
            return Ok(false);
        }
    }
    Ok(true)
}

fn paths(root: &Path, cancel: &CancellationToken) -> Result<Vec<String>, Fault> {
    fn visit(
        root: &Path,
        relative: &Path,
        result: &mut Vec<String>,
        cancel: &CancellationToken,
    ) -> Result<(), Fault> {
        for entry in std::fs::read_dir(root.join(relative)).map_err(storage)? {
            check(cancel)?;
            let entry = entry.map_err(storage)?;
            let name = entry.file_name();
            if name == MARKER || name == ".sailry-used" || name == "__pycache__" {
                continue;
            }
            let path = relative.join(&name);
            let metadata = entry.path().symlink_metadata().map_err(storage)?;
            if metadata.is_dir() && !metadata.is_symlink() {
                visit(root, &path, result, cancel)?;
            } else if metadata.is_file() || metadata.is_symlink() {
                if result.len() >= MAX_FILES {
                    return Err(Fault::invalid("Python environment contains too many files"));
                }
                result.push(
                    path.to_str()
                        .ok_or_else(|| Fault::invalid("Python environment path is not Unicode"))?
                        .replace('\\', "/"),
                );
            } else {
                return Err(Fault::invalid("Python environment contains a special file"));
            }
        }
        Ok(())
    }
    let mut result = Vec::new();
    visit(root, Path::new(""), &mut result, cancel)?;
    result.sort();
    Ok(result)
}

pub(super) fn hash(path: &Path, cancel: &CancellationToken) -> Result<String, Fault> {
    let mut file = std::fs::File::open(path).map_err(storage)?;
    let before = file.metadata().map_err(storage)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        check(cancel)?;
        let count = file.read(&mut buffer).map_err(storage)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > before.len() {
            return Err(Fault::invalid(
                "Python environment changed during verification",
            ));
        }
        digest.update(&buffer[..count]);
    }
    if total != before.len()
        || Stamp::new(&before) != Stamp::new(&file.metadata().map_err(storage)?)
    {
        return Err(Fault::invalid(
            "Python environment changed during verification",
        ));
    }
    Ok(format!("{:x}", digest.finalize()))
}
