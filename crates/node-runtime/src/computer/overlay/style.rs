//! agent-desktop v1.2.0's fresh session manifest supports custom PNG cursors.
//! Configure before MCP starts so the first Present control draws without its Enable greeting.
//! Its X11 renderer only supports the built-in arrow; macOS and Windows accept PNGs.
use super::{Path, Value};
use serde_json::json;

#[cfg(any(target_os = "macos", target_os = "windows"))]
const CURSOR: &[u8] = include_bytes!("../../../../../assets/computer/cursor.png");

pub(super) async fn configure(home: &Path, session: &str) -> Result<(), String> {
    let mut manifest = super::manifest(home, session)
        .await
        .filter(|manifest| {
            manifest.get("id").and_then(Value::as_str) == Some(session)
                && manifest.get("ended_at").is_none_or(Value::is_null)
        })
        .ok_or("INVALID_SESSION")?;
    let directory = home.join("sessions").join(session);
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    let style = {
        let cursor = directory.join("cursor.png");
        tokio::fs::write(&cursor, CURSOR)
            .await
            .map_err(|_| "CURSOR_WRITE")?;
        json!({
            "accent": "#4299FF",
            "images": {
                "arrow": {
                    "path": cursor,
                    "hotspot": {"x": 16, "y": 16}
                }
            }
        })
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let style = json!({"size": 0.75, "rim": "#FFFFFF"});
    manifest["cursor_overlay"] = json!({
        "enabled": true,
        "style": style
    });
    let content = serde_json::to_vec(&manifest).map_err(|_| "INVALID_SESSION")?;
    let temporary = tempfile::NamedTempFile::new_in(&directory).map_err(|_| "CONFIG_WRITE")?;
    tokio::fs::write(temporary.path(), content)
        .await
        .map_err(|_| "CONFIG_WRITE")?;
    temporary
        .persist(directory.join("session.json"))
        .map_err(|_| "CONFIG_WRITE")?;
    Ok(())
}
