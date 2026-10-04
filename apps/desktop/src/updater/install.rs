//! Whole-application installation through the pinned updater, never a binary-only swap.
use super::{Failure, Result, cached, config::Config, manifest, package, transfer::Staged};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

pub(super) fn installed() -> Result<PathBuf> {
    let executable = std::env::current_exe()
        .map_err(Failure::io)?
        .canonicalize()
        .map_err(Failure::io)?;
    #[cfg(target_os = "macos")]
    {
        let root = executable
            .ancestors()
            .find(|path| path.extension().is_some_and(|extension| extension == "app"))
            .ok_or_else(|| {
                Failure::new(
                    "updates_bundle_required",
                    "updates require a packaged macOS application",
                )
            })?;
        if root.join("Contents/MacOS/sailry-desktop") != executable {
            return Err(Failure::new(
                "updates_bundle_required",
                "the executable is outside the expected application bundle",
            ));
        }
        Ok(root.to_path_buf())
    }
    #[cfg(target_os = "windows")]
    {
        executable.parent().map(Path::to_path_buf).ok_or_else(|| {
            Failure::new(
                "updates_bundle_required",
                "the installed application has no bundle directory",
            )
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err(Failure::new(
            "updates_platform",
            "desktop updates are supported on macOS and Windows",
        ))
    }
}

pub(super) fn separate_data(installed: &Path, profile: &Path, cache: &Path) -> Result<()> {
    let installed = installed.canonicalize().map_err(Failure::io)?;
    let profile = profile.canonicalize().map_err(Failure::io)?;
    // The cache may not exist yet, and its parent can be a symlink. Resolve its
    // nearest existing ancestor without creating anything inside an app bundle.
    let ancestor = cache
        .ancestors()
        .find(|path| fs::symlink_metadata(path).is_ok())
        .ok_or_else(|| {
            Failure::new(
                "updates_io_failed",
                "the update cache has no existing parent",
            )
        })?;
    let ancestor = ancestor.canonicalize().map_err(Failure::io)?;
    if profile.starts_with(&installed) || ancestor.starts_with(&installed) {
        return Err(Failure::new(
            "updates_profile_inside",
            "application replacement cannot include the data profile or controller recovery cache",
        ));
    }
    Ok(())
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn replace(staged: &Staged, config: &Config, installed: &Path) -> Result<PathBuf> {
    manifest::revalidate(&staged.selection, config)?;
    package::verify_archive(&staged.archive, &staged.selection.release, &config.keys)?;
    let parent = installed.parent().ok_or_else(|| {
        Failure::new(
            "updates_bundle_required",
            "the installed bundle has no parent",
        )
    })?;
    let backup = tempfile::Builder::new()
        .prefix(".sailry-update-backup-")
        .tempdir_in(parent)
        .map_err(Failure::io)?;
    let original = backup.path().join("Sailry.app");
    copy_tree(installed, &original)?;
    // Retain a complete, independent original before the library's best-effort
    // rollback. A rollback failure must not cause its temporary stash to be the
    // only remaining copy of the installed resources or code signature.
    let backup = backup.keep();
    let release = staged.selection.release.clone();
    let selected = release.clone();
    let asset_name = release.name.clone();
    let source = self_update::Release::builder()
        .version(&release.version)
        .asset(self_update::ReleaseAsset::new(&release.name, &release.url))
        .build()
        .map_err(|error| Failure::new("updates_package_invalid", error.to_string()))?;
    let updater = self_update::backends::custom::Update::configure()
        .source(cached::Source(source))
        .target(&config.target)
        .bin_name("sailry-desktop")
        .current_version(&config.version)
        .bundle_path_in_archive(&release.bundle)
        .bundle_install_path(installed)
        .no_confirm(true)
        .show_output(false)
        .show_download_progress(false)
        .http_client(Arc::new(cached::Client {
            url: release.url.clone(),
            path: staged.archive.clone(),
        }))
        .asset_matcher(move |assets| {
            assets
                .iter()
                .find(|asset| asset.name() == asset_name)
                .cloned()
        })
        .verify_checksum(self_update::Checksum::Sha256(release.sha256.clone()))
        .verifying_keys(config.keys.clone())
        .verify_binary(move |root| {
            package::validate(root, &selected)
                .map_err(|error| self_update::Error::verification_rejected(error.to_string()))
        })
        .build()
        .map_err(|error| Failure::new("updates_install_failed", error.to_string()))?;
    updater.update().map_err(|error| {
        Failure::new(
            "updates_install_uncertain",
            format!(
                "application installation failed; original retained at {}: {error}",
                backup.display()
            ),
        )
    })?;
    Ok(backup)
}

#[cfg(any(target_os = "macos", test))]
fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source).map_err(Failure::io)?;
    if metadata.file_type().is_symlink() {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(fs::read_link(source).map_err(Failure::io)?, destination)
                .map_err(Failure::io)?;
        }
        #[cfg(not(unix))]
        {
            return Err(Failure::new(
                "updates_package_invalid",
                "a macOS bundle requires Unix symlink support",
            ));
        }
    } else if metadata.is_dir() {
        fs::create_dir(destination).map_err(Failure::io)?;
        for entry in fs::read_dir(source).map_err(Failure::io)? {
            let entry = entry.map_err(Failure::io)?;
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        }
        fs::set_permissions(destination, metadata.permissions()).map_err(Failure::io)?;
    } else if metadata.is_file() {
        fs::copy(source, destination).map_err(Failure::io)?;
    } else {
        return Err(Failure::new(
            "updates_package_invalid",
            "the installed bundle contains a non-file resource",
        ));
    }
    Ok(())
}

pub(super) fn restart(executable: &Path, arguments: &[std::ffi::OsString]) -> Result<()> {
    std::process::Command::new(executable)
        .args(arguments)
        .spawn()
        .map_err(|error| Failure::new("updates_restart_failed", error.to_string()))?;
    Ok(())
}
