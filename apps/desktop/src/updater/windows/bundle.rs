//! Portable filesystem operations; the process/DLL boundary lives in `process`.

use std::{fs, path::Path};

pub(super) fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(target_os = "windows"))]
    metadata.file_type().is_symlink()
}

pub(super) fn validate_tree(root: &Path) -> Result<(), String> {
    for entry in fs::read_dir(root)
        .map_err(|error| format!("cannot read application bundle {}: {error}", root.display()))?
    {
        let path = entry
            .map_err(|error| format!("cannot read a bundle entry: {error}"))?
            .path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("cannot inspect bundle entry {}: {error}", path.display()))?;
        if is_link(&metadata) {
            return Err(format!(
                "Windows bundle contains a link: {}",
                path.display()
            ));
        }
        if metadata.is_dir() {
            validate_tree(&path)?;
        } else if !metadata.is_file() {
            return Err(format!(
                "bundle contains a non-file entry: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

pub(super) fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir(destination).map_err(|error| {
        format!(
            "cannot create helper bundle {}: {error}",
            destination.display()
        )
    })?;
    #[cfg(target_os = "windows")]
    super::process::protect(destination)?;
    for entry in fs::read_dir(source)
        .map_err(|error| format!("cannot read the installed bundle: {error}"))?
    {
        let entry = entry.map_err(|error| format!("cannot read an installed entry: {error}"))?;
        let path = entry.path();
        let target = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            format!("cannot inspect installed entry {}: {error}", path.display())
        })?;
        if is_link(&metadata) {
            return Err(format!(
                "installed Windows bundle contains a link: {}",
                path.display()
            ));
        }
        if metadata.is_dir() {
            copy_tree(&path, &target)?;
        } else if metadata.is_file() {
            fs::copy(&path, &target)
                .map_err(|error| format!("cannot copy helper entry {}: {error}", path.display()))?;
            #[cfg(target_os = "windows")]
            super::process::protect(&target)?;
        } else {
            return Err(format!(
                "installed bundle contains a non-file entry: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

/// Reuse the pinned library's directory move/rollback primitive. The backup is
/// deliberately retained, including after success; no user data is deleted.
pub(super) fn replace(staged: &Path, installed: &Path, backup: &Path) -> Result<(), String> {
    match fs::symlink_metadata(backup) {
        Ok(_) => return Err("update backup already exists".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("cannot inspect the update backup: {error}")),
    }
    match self_update::Move::from_source(staged)
        .replace_using_temp(backup)
        .to_dest(installed)
    {
        Ok(()) => Ok(()),
        Err(error) => {
            if backup.exists() {
                Err(format!(
                    "installation failed and rollback is uncertain: {error}; original bundle retained at {}",
                    backup.display()
                ))
            } else if installed.is_dir() {
                Err(format!(
                    "installation failed; the original bundle was restored: {error}"
                ))
            } else {
                Err(format!(
                    "installation failed and the installed bundle is unavailable: {error}"
                ))
            }
        }
    }
}
