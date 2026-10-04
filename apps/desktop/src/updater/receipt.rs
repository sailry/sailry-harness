use super::{Failure, Result};
use serde::Deserialize;
use std::{fs, io::Write as _, path::Path};

#[derive(Deserialize)]
struct Receipt {
    version: u32,
    result: String,
    message: Option<String>,
    uncertain: Option<bool>,
    installed: Option<bool>,
}

pub(super) fn read(directory: &Path) -> Result<Option<&'static str>> {
    let path = directory.join("receipt.json");
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Failure::io(error)),
    };
    if bytes.len() > 64 * 1024 {
        return Err(Failure::new(
            "updates_receipt_unread",
            "the update receipt exceeds its size limit",
        ));
    }
    let receipt: Receipt = serde_json::from_slice(&bytes)
        .map_err(|_| Failure::new("updates_receipt_unread", "the update receipt is invalid"))?;
    if receipt.version != 1 || !matches!(receipt.result.as_str(), "success" | "failure") {
        return Err(Failure::new(
            "updates_receipt_unread",
            "the update receipt has an unsupported definition",
        ));
    }
    let receipts = directory.join("receipts");
    fs::create_dir_all(&receipts).map_err(Failure::io)?;
    let archived = tempfile::Builder::new()
        .prefix("receipt-")
        .suffix(".json")
        .tempfile_in(&receipts)
        .map_err(Failure::io)?;
    fs::rename(&path, archived.path()).map_err(Failure::io)?;
    archived.keep().map_err(|error| Failure::io(error.error))?;
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
    } else {
        Ok(Some("updates_completed"))
    }
}

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
