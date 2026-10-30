use super::super::format;
use sailry_protocol::{ErrorCode, Fault};
use sentry::protocol::Attachment;
use serde_json::{Map, Value};
use std::{
    io::{Read as _, Seek as _, SeekFrom},
    path::PathBuf,
};

const LINES: usize = 500;

pub(super) fn failure() -> Fault {
    Fault::new(
        ErrorCode::Unavailable,
        "diagnostic reporting is unavailable",
    )
}

pub(super) fn attachment(path: Option<PathBuf>) -> Result<Attachment, Fault> {
    let mut buffer = String::new();
    if let Some(path) = path {
        let mut file = std::fs::File::open(path).map_err(|_| failure())?;
        let length = file.metadata().map_err(|_| failure())?.len();
        let start = length.saturating_sub(super::super::MAX_FILE_BYTES as u64);
        file.seek(SeekFrom::Start(start)).map_err(|_| failure())?;
        let mut bytes = Vec::new();
        file.take(super::super::MAX_FILE_BYTES as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| failure())?;
        let text = String::from_utf8_lossy(&bytes);
        let text = if start > 0 {
            text.split_once('\n').map_or("", |(_, tail)| tail)
        } else {
            text.as_ref()
        };
        let mut lines: Vec<_> = text.lines().rev().take(LINES).collect();
        lines.reverse();
        for line in lines {
            if let Some(fields) = parse(line) {
                buffer.push_str(&Value::Object(fields).to_string());
                buffer.push('\n');
            }
        }
    }
    Ok(Attachment {
        buffer: buffer.into_bytes(),
        filename: "diagnostics.jsonl".into(),
        content_type: Some("application/x-ndjson".into()),
        ..Default::default()
    })
}

fn parse(line: &str) -> Option<Map<String, Value>> {
    let mut header = line.splitn(4, ' ');
    let timestamp = header.next()?;
    chrono::DateTime::parse_from_rfc3339(timestamp).ok()?;
    let level = header.next()?;
    if !matches!(level, "ERROR" | "WARN" | "INFO" | "DEBUG") {
        return None;
    }
    if !matches!(header.next()?, "host" | "desktop") {
        return None;
    }
    let mut rest = header.next().unwrap_or_default();
    let mut fields = Map::new();
    while !rest.is_empty() {
        let (key, tail) = rest.split_once('=')?;
        if key.is_empty()
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        {
            return None;
        }
        let (value, used) = if tail.starts_with('"') {
            let mut parser = serde_json::Deserializer::from_str(tail).into_iter::<Value>();
            let value = parser.next()?.ok()?;
            (value, parser.byte_offset())
        } else {
            let end = tail.find(' ').unwrap_or(tail.len());
            let value = &tail[..end];
            (
                serde_json::from_str(value).unwrap_or_else(|_| value.into()),
                end,
            )
        };
        if fields.insert(key.into(), value).is_some() {
            return None;
        }
        rest = tail.get(used..)?;
        if !rest.is_empty() {
            rest = rest.strip_prefix(' ')?;
        }
    }
    let mut fields = format::remote(&fields);
    fields.insert("level".into(), level.into());
    fields.insert("timestamp".into(), timestamp.into());
    Some(fields)
}
