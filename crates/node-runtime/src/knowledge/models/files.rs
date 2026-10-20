use super::{Model, catalog, unavailable};
use cap_fs_ext::DirExt;
use cap_std::fs::{Dir, DirBuilder};
pub(super) use sailry_model_assets::verified::Stamp;
use sailry_protocol::{Fault, models::Definition};
use std::path::{Path, PathBuf};

pub(super) fn open(profile: &Path) -> Result<(PathBuf, Dir), Fault> {
    let profile = profile.canonicalize().map_err(|_| unavailable())?;
    let mut directory =
        Dir::open_ambient_dir(&profile, cap_std::ambient_authority()).map_err(|_| unavailable())?;
    for name in ["models", "embedding"] {
        let builder = DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use cap_std::fs::DirBuilderExt;
            let mut builder = builder;
            builder.mode(0o700);
            builder
        };
        match directory.create_dir_with(name, &builder) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(_) => return Err(unavailable()),
        }
        directory = directory
            .open_dir_nofollow(name)
            .map_err(|_| unavailable())?;
    }
    Ok((profile.join("models/embedding"), directory))
}

pub(super) fn directory(root: &Path, model: Model) -> PathBuf {
    root.join(model.key().as_ref()).join(folder(model))
}

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

pub(super) fn stamp(root: &Path, model: Model, definition: &Definition) -> Option<Stamp> {
    let marker = matches!(model, Model::Registered { .. })
        .then(|| digest(definition))
        .transpose()
        .ok()?;
    sailry_model_assets::verified::stamp(
        &directory(root, model),
        &files(definition),
        marker.as_deref(),
    )
}

pub(super) fn verified(root: &Path, model: Model, definition: &Definition) -> Option<Stamp> {
    let marker = matches!(model, Model::Registered { .. })
        .then(|| digest(definition))
        .transpose()
        .ok()?;
    sailry_model_assets::verified::verified(
        &directory(root, model),
        &files(definition),
        marker.as_deref(),
    )
}

pub(super) fn remove(root: &Dir, model: Model) -> Result<(), Fault> {
    let directory = match root.open_dir_nofollow(model.key().as_ref()) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(unavailable()),
    };
    let revision = folder(model);
    match directory.symlink_metadata(&revision) {
        Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        _ => return Err(unavailable()),
    }
    directory
        .remove_dir_all(&revision)
        .map_err(|_| unavailable())
}

fn folder(model: Model) -> String {
    catalog::builtin(model).map_or_else(|| "files".into(), |definition| definition.source.revision)
}

fn digest(definition: &Definition) -> Result<String, Fault> {
    let body = serde_json::to_vec(definition).map_err(|_| unavailable())?;
    Ok(blake3::hash(&body).to_hex().to_string())
}

pub(super) fn mark(root: &Dir, model: Model, definition: &Definition) -> Result<(), Fault> {
    if !matches!(model, Model::Registered { .. }) {
        return Ok(());
    }
    let directory = root
        .open_dir_nofollow(model.key().as_ref())
        .map_err(|_| unavailable())?
        .open_dir_nofollow(folder(model))
        .map_err(|_| unavailable())?;
    sailry_model_assets::verified::write_marker(&directory, &digest(definition)?)
        .map_err(|_| unavailable())
}
