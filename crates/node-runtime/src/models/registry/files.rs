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
    for name in Kind::NODE.into_iter().filter_map(folder) {
        crate::profile::private_directory(&root.join(name)).map_err(super::failure)?;
        crate::files::path::root(&root.join(name))?;
    }
    Ok(root)
}

fn folder(kind: Kind) -> Option<&'static str> {
    match kind {
        Kind::Ocr => Some("ocr"),
        Kind::Embedding => Some("embedding"),
        Kind::Decision => Some("decision"),
        Kind::Speech => None,
    }
}

pub(in crate::models) fn directory(
    root: &Path,
    id: &str,
    definition: &Definition,
) -> Result<PathBuf, Fault> {
    if !sailry_protocol::models::valid_id(id) {
        return Err(Fault::invalid("invalid local model identity"));
    }
    let kind = definition.format.kind();
    let folder = folder(kind).ok_or_else(|| Fault::invalid("speech belongs to the controller"))?;
    let directory = root.join(folder).join(id);
    Ok(if kind == Kind::Embedding {
        directory.join(if catalog::builtin(id) {
            definition.source.revision.as_str()
        } else {
            "files"
        })
    } else {
        directory
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

/// Returns marked files whose identity is cached or recorded, without reading contents.
pub(in crate::models) fn known(
    path: &Path,
    definition: &Definition,
    cached: Option<&Stamp>,
) -> Option<Stamp> {
    let marker = catalog::id(definition);
    let stamp = verified::stamp(path, &assets(definition), Some(&marker))?;
    (cached == Some(&stamp) || verified::recorded(path, &marker, &stamp)).then_some(stamp)
}

/// Compares a captured stamp with the marked files without reading their contents.
pub(in crate::models) fn unchanged(resource: &super::Resource) -> bool {
    let files = assets(&resource.definition);
    verified::stamp(
        &resource.directory,
        &files,
        Some(&catalog::id(&resource.definition)),
    )
    .or_else(|| {
        previous(
            &resource.directory,
            &resource.id,
            &resource.definition,
            &files,
        )
    })
    .as_ref()
        == Some(&resource.stamp)
}

/// Verifies files, hashing only when neither the cache nor a receipt covers them.
pub(in crate::models) fn stamp(
    path: &Path,
    id: &str,
    definition: &Definition,
    cached: Option<&Stamp>,
) -> Option<Stamp> {
    if let Some(stamp) = known(path, definition, cached) {
        return Some(stamp);
    }
    let files = assets(definition);
    let marker = catalog::id(definition);
    if verified::stamp(path, &files, Some(&marker)).is_some() {
        probe::hashing(path);
        return verified::verified(path, &files, Some(&marker));
    }
    let before = previous(path, id, definition, &files)?;
    if cached == Some(&before) {
        return Some(before);
    }
    probe::hashing(path);
    match verified::mark(path, &files, &marker, &before) {
        Ok(stamp) => Some(stamp),
        // A read-only profile can keep using its verified installed files. A
        // marker rewrite is not a reason to require another model download.
        Err(sailry_model_assets::Error::Storage(_)) => {
            previous(path, id, definition, &files).filter(|stamp| *stamp == before)
        }
        Err(_) => None,
    }
}

fn previous(path: &Path, id: &str, definition: &Definition, files: &[File<'_>]) -> Option<Stamp> {
    let marker = if catalog::builtin(id) {
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
    verified::stamp(path, files, marker.as_deref())
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

/// Test observation of content hashing; release builds compile it away.
pub(in crate::models) mod probe {
    #[cfg(test)]
    use std::{
        path::PathBuf,
        sync::{Mutex, mpsc},
    };

    #[cfg(test)]
    pub(in crate::models) static HASHED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
    #[cfg(test)]
    type Gate = (PathBuf, mpsc::Sender<()>, mpsc::Receiver<()>);
    /// Pauses hashing of one directory: reports entry, then waits for release.
    #[cfg(test)]
    pub(in crate::models) static GATE: Mutex<Option<Gate>> = Mutex::new(None);

    pub(super) fn hashing(_path: &std::path::Path) {
        #[cfg(test)]
        {
            HASHED.lock().unwrap().push(_path.to_owned());
            let gate = {
                let mut gate = GATE.lock().unwrap();
                gate.take_if(|(path, _, _)| path == _path)
            };
            if let Some((_, entered, release)) = gate {
                let _ = entered.send(());
                let _ = release.recv();
            }
        }
    }
}

#[cfg(test)]
mod tests;
