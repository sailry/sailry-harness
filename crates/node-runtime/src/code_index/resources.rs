//! Application-shipped parsers are shared by the Node; runtime never downloads them.
use super::parser::Parser;
use sailry_protocol::{ErrorCode, Fault};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub(super) const PACK: &str = "1.18.0";
pub(super) const TREE_SITTER: &str = "0.26.13";
const SOURCE: &str = "3bdfbbf639f8e0fdec61c7f957bcb3da7671cb208b18aefbd721874af2017b28";

pub(super) struct Resources {
    pub parser: Arc<Parser>,
    pub fingerprint: String,
}

#[derive(Deserialize)]
struct Manifest {
    version: u16,
    language_pack: String,
    tree_sitter: String,
    source_sha256: String,
    languages: Vec<String>,
}

impl Resources {
    pub(super) fn open(directory: &Path) -> Result<Self, Fault> {
        let bytes =
            std::fs::read(directory.join("code-parsers.json")).map_err(|_| unavailable())?;
        if bytes.len() > 256 * 1024 {
            return Err(unavailable());
        }
        let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
        if manifest.version != 1
            || manifest.language_pack != PACK
            || manifest.tree_sitter != TREE_SITTER
            || manifest.source_sha256 != SOURCE
            || manifest.languages.len() != 371
        {
            return Err(unavailable());
        }
        Ok(Self {
            parser: Arc::new(Parser::open(directory)),
            // Increment the rule marker only when extraction/range rules change.
            fingerprint: blake3::hash(
                format!("code-index-v1:{PACK}:{TREE_SITTER}:{SOURCE}:embedded-1:data-1").as_bytes(),
            )
            .to_hex()
            .to_string(),
        })
    }

    pub(super) fn bundled() -> Result<Self, Fault> {
        Self::open(&directory()?)
    }
}

fn directory() -> Result<PathBuf, Fault> {
    // Build/test invocations can select the same verified, shared resource directory.
    if let Some(directory) = std::env::var_os("SAILRY_CODE_PARSERS") {
        return Ok(directory.into());
    }
    let executable = std::env::current_exe().map_err(|_| unavailable())?;
    let parent = executable.parent().ok_or_else(unavailable)?;
    if parent.file_name().is_some_and(|name| name == "MacOS")
        && parent
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == "Contents")
    {
        return Ok(parent.parent().unwrap().join("Resources/code-parsers"));
    }
    let parent = if parent
        .file_name()
        .is_some_and(|name| name == "deps" || name == "examples")
    {
        parent.parent().ok_or_else(unavailable)?
    } else {
        parent
    };
    Ok(parent.join("code-parsers"))
}

fn unavailable() -> Fault {
    Fault::new(
        ErrorCode::NotConfigured,
        "bundled code parsers are unavailable",
    )
}
