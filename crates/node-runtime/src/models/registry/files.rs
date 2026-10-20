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
    for name in ["ocr", "embedding"] {
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

pub(in crate::models) fn marker(id: &str, definition: &Definition) -> Option<String> {
    match definition.format.kind() {
        Kind::Embedding if catalog::builtin(id) => None,
        Kind::Embedding if id.starts_with("model-") => Some(
            blake3::hash(
                &serde_json::to_vec(definition).expect("model definition is serializable"),
            )
            .to_hex()
            .to_string(),
        ),
        _ => Some(catalog::id(definition)),
    }
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

pub(in crate::models) fn stamp(path: &Path, id: &str, definition: &Definition) -> Option<Stamp> {
    verified::stamp(path, &assets(definition), marker(id, definition).as_deref())
}

pub(in crate::models) fn receipt(path: &Path, id: &str, definition: &Definition) -> Option<Stamp> {
    verified::verified(path, &assets(definition), marker(id, definition).as_deref())
}

pub(in crate::models) fn mark(
    path: &Path,
    id: &str,
    definition: &Definition,
    expected: &Stamp,
) -> Result<Stamp, Fault> {
    match marker(id, definition) {
        Some(marker) => {
            verified::mark(path, &assets(definition), &marker, expected).map_err(super::failure)
        }
        None => receipt(path, id, definition)
            .filter(|actual| actual == expected)
            .ok_or_else(|| Fault::invalid("model assets changed during validation")),
    }
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
