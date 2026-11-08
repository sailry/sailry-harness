use super::{Failure, Result};
use serde::Deserialize;
#[cfg(any(target_os = "macos", test))]
use std::io::Write as _;
use std::{
    fs,
    io::Read as _,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
struct Receipt {
    version: u32,
    result: String,
    application_version: Option<String>,
    message: Option<String>,
    uncertain: Option<bool>,
    installed: Option<bool>,
    backup: Option<PathBuf>,
}

pub(super) fn read(directory: &Path, installed: Option<&Path>) -> Result<Option<&'static str>> {
    let path = directory.join("receipt.json");
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Failure::io(error)),
    };
    let receipts = directory.join("receipts");
    fs::create_dir_all(&receipts).map_err(Failure::io)?;
    let archived = tempfile::Builder::new()
        .prefix("receipt-")
        .suffix(".json")
        .tempfile_in(&receipts)
        .map_err(Failure::io)?;
    fs::rename(&path, archived.path()).map_err(Failure::io)?;
    archived.keep().map_err(|error| Failure::io(error.error))?;
    let mut bytes = Vec::new();
    file.take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(Failure::io)?;
    if bytes.len() > 64 * 1024 {
        return Err(Failure::new(
            "updates_receipt_unread",
            "the update receipt exceeds its size limit",
        ));
    }
    let receipt: Receipt = serde_json::from_slice(&bytes)
        .map_err(|_| Failure::new("updates_receipt_unread", "the update receipt is invalid"))?;
    if receipt.version != 1
        || !matches!(receipt.result.as_str(), "success" | "failure")
        || receipt.result == "success"
            && receipt
                .application_version
                .as_ref()
                .is_none_or(|version| semver::Version::parse(version).is_err())
    {
        return Err(Failure::new(
            "updates_receipt_unread",
            "the update receipt has an unsupported definition",
        ));
    }
    if receipt.result == "failure" {
        Err(Failure::new(
            if receipt.installed == Some(true) {
                "updates_restart_failed"
            } else if receipt.uncertain == Some(true) {
                "updates_install_uncertain"
            } else {
                "updates_install_failed"
            },
            receipt
                .message
                .unwrap_or_else(|| "the prior update did not complete".into()),
        ))
    } else if receipt.uncertain == Some(true) {
        Err(Failure::new(
            "updates_install_uncertain",
            receipt
                .message
                .unwrap_or_else(|| "the prior installation is unconfirmed".into()),
        ))
    } else if receipt.application_version.as_deref() != Some(env!("CARGO_PKG_VERSION")) {
        Err(Failure::new(
            "updates_restart_failed",
            "the recorded update does not match the running application",
        ))
    } else {
        if let Some(backup) = receipt.backup {
            cleanup(&backup, installed);
        }
        Ok(Some("updates_completed"))
    }
}

fn cleanup(backup: &Path, installed: Option<&Path>) {
    use sailry_node_runtime::logging::{Area, Entry, Phase};
    let result = (|| {
        let Some(installed) = installed else {
            return Ok(false);
        };
        if !backup.is_absolute()
            || !backup
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(".sailry-update-backup-"))
            || !fs::symlink_metadata(backup).is_ok_and(|metadata| metadata.is_dir())
            || !fs::symlink_metadata(backup.join("Sailry.app"))
                .is_ok_and(|metadata| metadata.is_dir())
        {
            return Ok(false);
        }
        let (Ok(backup), Ok(installed)) = (backup.canonicalize(), installed.canonicalize()) else {
            return Ok(false);
        };
        if backup.parent() != installed.parent() || backup == installed {
            return Ok(false);
        }
        fs::remove_dir_all(backup)?;
        Ok::<_, std::io::Error>(true)
    })();
    let mut entry = Entry::new(
        Area::Desktop,
        if matches!(result, Ok(true)) {
            Phase::Completed
        } else {
            Phase::Failed
        },
    );
    entry.operation = Some("desktop_update");
    entry.stage = Some("backup_cleanup");
    match result {
        Ok(true) => {
            entry.reason = Some("deleted");
            entry.emit();
        }
        Ok(false) => {
            entry.reason = Some("kept_unverified");
            entry.emit_warn();
        }
        Err(error) => {
            entry.reason = Some("delete_failed");
            entry.detail = Some(error.to_string());
            entry.emit_warn();
        }
    }
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn write(path: &Path, value: &serde_json::Value) -> Result<()> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| Failure::new("updates_io_failed", "update receipt serialization failed"))?;
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(Failure::io)?;
    file.write_all(&bytes).map_err(Failure::io)?;
    file.sync_all().map_err(Failure::io)
}
