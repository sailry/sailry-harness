use sailry_link::{EndpointAddr, LinkHandle, rendezvous::Relay};
use sailry_protocol::NodeId;
use std::{
    fs::{self, File},
    io::{BufRead, Read},
    path::Path,
    time::Duration,
};

use super::transfer::{Result, redacted};

const MARKER: &str = "provider-copy-controller";

pub(super) fn check_directory(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => {
            let marker = fs::symlink_metadata(path.join(MARKER))
                .map_err(|_| "existing_directory_not_owned_by_helper")?;
            if !marker.is_file() || marker.is_symlink() || marker.len() != 0 {
                return Err("invalid_controller_marker");
            }
            for entry in fs::read_dir(path).map_err(|_| "controller_directory_unavailable")? {
                let entry = entry.map_err(|_| "controller_directory_unavailable")?;
                let kind = entry
                    .file_type()
                    .map_err(|_| "controller_directory_unavailable")?;
                if !kind.is_file()
                    || !matches!(
                        entry.file_name().to_str(),
                        Some(MARKER | "link.key" | "peers.json" | "node.lock" | "copy.once")
                    )
                {
                    return Err("controller_directory_contains_unexpected_entry");
                }
            }
        }
        _ => return Err("new_controller_directory_required"),
    }
    Ok(false)
}

pub(super) fn mark_directory(path: &Path) -> Result<()> {
    File::create_new(path.join(MARKER))
        .and_then(|file| file.sync_all())
        .map_err(|_| "controller_marker_unavailable")
}

pub(super) fn reserve_copy(directory: &Path) -> Result<()> {
    File::create_new(directory.join("copy.once"))
        .and_then(|file| file.sync_all())
        .map_err(|_| "copy_already_attempted_or_marker_unavailable_no_replay")
}

pub(super) async fn connect(
    handle: &LinkHandle,
    expected: NodeId,
    pair: bool,
    service: &str,
    role: &'static str,
    platform: &str,
) -> Result<EndpointAddr> {
    if NodeId(*handle.address().id.as_bytes()) == expected {
        return Err("controller_cannot_be_source_or_target");
    }
    if pair {
        println!("status={role}_pairing_input_required");
        let input = tokio::task::spawn_blocking(read_invitation)
            .await
            .map_err(|_| "pairing_input_unavailable")??;
        let address = if input.bytes().all(|byte| byte.is_ascii_digit()) && input.len() == 6 {
            let relay = Relay::new(service).map_err(|_| "invalid_pairing_service")?;
            let request = sailry_link::rendezvous::RequestId::default();
            relay.pair(handle, &input, &request).await
        } else {
            handle.pair(&input).await
        }
        .map_err(|_| "authorized_pairing_failed_no_profile_fallback")?;
        let paired = NodeId(*address.id.as_bytes());
        if paired != expected {
            let _ = handle.set_trust(paired, false).await;
            return Err("invitation_identity_mismatch_no_business_commands_sent");
        }
    }
    let address = handle
        .peers()
        .await
        .map_err(|_| "paired_peers_unavailable")?
        .into_iter()
        .find(|address| NodeId(*address.id.as_bytes()) == expected)
        .ok_or("authorized_app_pairing_required_no_profile_fallback")?;
    let info = tokio::time::timeout(Duration::from_secs(45), handle.inspect(&address))
        .await
        .map_err(|_| "authenticated_peer_inspection_timed_out")?
        .map_err(|_| "authenticated_peer_unavailable")?;
    if !info.execution || info.platform != platform {
        return Err("peer_execution_platform_mismatch");
    }
    info.compatibility()
        .map_err(|_| "peer_protocol_or_version_incompatible")?;
    println!("status={role}_authenticated node={}", redacted(expected));
    Ok(address)
}

fn read_invitation() -> Result<String> {
    let mut input = String::new();
    std::io::stdin()
        .lock()
        .take(8194)
        .read_line(&mut input)
        .map_err(|_| "pairing_input_unavailable")?;
    let input = input.trim_end_matches(['\r', '\n']);
    if input.is_empty() || input.len() > 8192 {
        return Err("invalid_pairing_input");
    }
    Ok(input.to_owned())
}
