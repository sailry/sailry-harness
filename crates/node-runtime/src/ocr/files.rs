pub(super) use sailry_model_assets::verified::Stamp;
use sailry_protocol::{Fault, models::Definition};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn files(definition: &Definition) -> Vec<sailry_model_assets::File<'_>> {
    definition
        .assets
        .iter()
        .map(|asset| sailry_model_assets::File {
            name: &asset.name,
            size: asset.bytes,
            sha256: &asset.sha256,
        })
        .collect()
}

pub(super) fn stamp(path: &Path, definition: &Definition) -> Option<Stamp> {
    sailry_model_assets::verified::stamp(
        path,
        &files(definition),
        Some(&super::catalog::id(definition)),
    )
}

pub(super) fn candidate(path: &Path, definition: &Definition) -> Option<Stamp> {
    sailry_model_assets::verified::candidate(path, &files(definition))
}

pub(super) fn verified(path: &Path, definition: &Definition) -> Option<Stamp> {
    sailry_model_assets::verified::verified(
        path,
        &files(definition),
        Some(&super::catalog::id(definition)),
    )
}

pub(super) fn mark(path: &Path, definition: &Definition, expected: &Stamp) -> Result<Stamp, Fault> {
    sailry_model_assets::verified::mark(
        path,
        &files(definition),
        &super::catalog::id(definition),
        expected,
    )
    .map_err(|error| match error {
        sailry_model_assets::Error::InvalidAsset => {
            Fault::invalid("OCR assets changed during validation")
        }
        error => super::failure(error),
    })
}

pub(super) fn open(profile: &Path) -> Result<PathBuf, Fault> {
    let profile = profile.canonicalize().map_err(super::failure)?;
    for path in [profile.join("models"), profile.join("models/ocr")] {
        crate::profile::private_directory(&path).map_err(super::failure)?;
        let metadata = fs::symlink_metadata(&path).map_err(super::failure)?;
        if metadata.is_symlink() || !metadata.is_dir() {
            return Err(Fault::invalid("OCR storage must be a directory"));
        }
    }
    Ok(profile.join("models/ocr"))
}
