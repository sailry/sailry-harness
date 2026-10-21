use super::{Fault, catalog};
use cap_fs_ext::DirExt;
use sailry_model_assets::{
    File,
    verified::{self, Stamp},
};
use sailry_protocol::models::{Definition, Kind};
use std::path::{Path, PathBuf};

pub(in crate::models) fn root(profile: &Path) -> Result<PathBuf, Fault> {
    let profile = profile.canonicalize().map_err(super::failure)?;
    crate::profile::private_directory(&profile.join("models")).map_err(super::failure)?;
    let root = profile.join("models");
    crate::files::path::root(&root)?;
    for name in ["ocr", "embedding", "decision"] {
        crate::profile::private_directory(&root.join(name)).map_err(super::failure)?;
        crate::files::path::root(&root.join(name))?;
    }
    Ok(root)
}

pub(in crate::models) fn directory(
    root: &Path,
    id: &str,
    definition: &Definition,
) -> Result<PathBuf, Fault> {
    if !sailry_protocol::models::valid_id(id) {
        return Err(Fault::invalid("invalid local model identity"));
    }
    Ok(match definition.format.kind() {
        Kind::Ocr => root.join("ocr").join(id),
        Kind::Decision => root.join("decision").join(id),
        Kind::Embedding => root
            .join("embedding")
            .join(id)
            .join(if catalog::builtin(id) {
                definition.source.revision.as_str()
            } else {
                "files"
            }),
        Kind::Speech => return Err(Fault::invalid("speech belongs to the controller")),
    })
}

pub(in crate::models) fn assets(definition: &Definition) -> Vec<File<'_>> {
    definition
        .assets
        .iter()
        .map(|asset| File {
            name: &asset.name,
            size: asset.bytes,
            sha256: &asset.sha256,
        })
        .collect()
}

pub(in crate::models) fn stamp(
    path: &Path,
    id: &str,
    definition: &Definition,
    cached: Option<&Stamp>,
) -> Option<Stamp> {
    let files = assets(definition);
    let marker = catalog::id(definition);
    if let Some(stamp) = verified::stamp(path, &files, Some(&marker)) {
        return if cached == Some(&stamp) {
            Some(stamp)
        } else {
            verified::verified(path, &files, Some(&marker))
        };
    }
    let previous = if catalog::builtin(id) {
        None
    } else if id.starts_with("model-") && definition.format.kind() == Kind::Embedding {
        Some(
            blake3::hash(&serde_json::to_vec(definition).ok()?)
                .to_hex()
                .to_string(),
        )
    } else {
        return None;
    };
    let before = verified::stamp(path, &files, previous.as_deref())?;
    if cached == Some(&before) {
        return Some(before);
    }
    match verified::mark(path, &files, &marker, &before) {
        Ok(stamp) => Some(stamp),
        // A read-only profile can keep using its verified installed files. A
        // marker rewrite is not a reason to require another model download.
        Err(sailry_model_assets::Error::Storage(_)) => {
            verified::stamp(path, &files, previous.as_deref()).filter(|stamp| *stamp == before)
        }
        Err(_) => None,
    }
}

pub(in crate::models) fn mark(
    path: &Path,
    definition: &Definition,
    expected: &Stamp,
) -> Result<Stamp, Fault> {
    verified::mark(
        path,
        &assets(definition),
        &catalog::id(definition),
        expected,
    )
    .map_err(super::failure)
}

pub(in crate::models) fn remove(
    root: &Path,
    id: &str,
    definition: &Definition,
) -> Result<(), Fault> {
    let path = directory(root, id, definition)?;
    let parent = path
        .parent()
        .ok_or_else(|| Fault::invalid("model directory has no parent"))?;
    let directory = match crate::files::path::root(parent) {
        Ok(directory) => directory,
        Err(_) if !parent.exists() => return Ok(()),
        Err(error) => return Err(error),
    };
    let name = path
        .file_name()
        .ok_or_else(|| Fault::invalid("model directory has no name"))?;
    match directory.open_dir_nofollow(name) {
        Ok(_) => directory.remove_dir_all(name).map_err(super::failure),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(super::failure(error)),
    }
}

#[cfg(test)]
mod tests;
