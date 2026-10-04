//! Standard tool content is data, never executable UI or an alternate history.
use super::Presentation;
use crate::plugin::desktop::Navigation;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Content {
    pub version: u32,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Block {
    Text(Text),
    Notice {
        message: Navigation,
    },
    Table(Table),
    /// Original inline-data index of an authenticated image in this result.
    Image {
        index: usize,
    },
    Diff(Diff),
    File(File),
}

/// Literal output references avoid a second copy of streams in canonical history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Text {
    pub path: String,
    #[serde(default)]
    pub notices: Vec<Navigation>,
}

impl Text {
    pub fn read<'a>(&self, result: &'a Value) -> Option<&'a str> {
        payload(result)?.pointer(&self.path)?.as_str()
    }
}

/// A worktree file produced by a tool. Its turn supplies the Node and worktree.
/// Metadata is descriptive; opening always rechecks access on the execution Node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    pub path: String,
    pub name: Option<String>,
    pub mime: String,
    pub size: Option<u64>,
    pub revision: Option<String>,
}

impl File {
    pub fn valid(&self) -> bool {
        !self.path.is_empty()
            && self.path.len() <= 4096
            && !self.path.contains(['\\', ':'])
            && !self.path.chars().any(char::is_control)
            && self
                .path
                .split('/')
                .all(|part| !matches!(part, "" | "." | ".."))
            && self.name.as_ref().is_none_or(|name| {
                !name.trim().is_empty() && name.len() <= 512 && !name.chars().any(char::is_control)
            })
            && self.mime.len() <= 128
            && self
                .mime
                .split_once('/')
                .is_some_and(|(kind, subtype)| !kind.is_empty() && !subtype.is_empty())
            && self
                .mime
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"/.-+".contains(&byte))
            && self.revision.as_ref().is_none_or(|revision| {
                !revision.is_empty()
                    && revision.len() <= 128
                    && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
    }

    pub fn label(&self) -> &str {
        self.name
            .as_deref()
            .unwrap_or_else(|| self.path.rsplit('/').next().unwrap_or(&self.path))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    pub columns: Vec<String>,
    /// Literal cell text; nested objects, markup and expressions are not interpreted.
    pub rows: Vec<Vec<String>>,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diff {
    /// Display and syntax context only; this does not grant file access.
    pub path: String,
    pub text: String,
    #[serde(default)]
    pub truncated: bool,
}

impl Content {
    pub fn valid(&self) -> bool {
        let mut images = std::collections::BTreeSet::new();
        self.version == 1
            && !self.blocks.is_empty()
            && self.blocks.len() <= 16
            && self.blocks.iter().all(|block| match block {
                Block::Text(text) => {
                    super::display::valid_pointer(&text.path)
                        && text.notices.len() <= 8
                        && text.notices.iter().all(Navigation::valid)
                }
                Block::Notice { message } => message.valid(),
                Block::Table(table) => {
                    !table.columns.is_empty()
                        && table.columns.len() <= 64
                        && table.columns.iter().all(|column| column.len() <= 1024)
                        && table.rows.len() <= 1000
                        && table.rows.iter().all(|row| {
                            row.len() == table.columns.len()
                                && row.iter().all(|cell| cell.len() <= 16 * 1024)
                        })
                }
                Block::Image { index } => images.insert(*index),
                Block::File(file) => file.valid(),
                Block::Diff(diff) => {
                    !diff.path.is_empty()
                        && diff.path.len() <= 4096
                        && !diff.path.chars().any(char::is_control)
                        && diff.text.lines().count() <= 4096
                }
            })
            && serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() <= MAX_BYTES)
    }

    pub fn has_visible(&self, result: &Value) -> bool {
        self.blocks.iter().any(|block| match block {
            Block::Text(text) => {
                !text.notices.is_empty() || text.read(result).is_some_and(|value| !value.is_empty())
            }
            _ => true,
        })
    }
}

impl Presentation {
    /// Native tools return `sailry_content`; ADK wraps MCP structuredContent in `output`.
    /// Invalid content falls back to the unchanged authoritative result.
    pub fn content(self, result: &Value) -> Option<Content> {
        if self != Self::Content || fault(result) {
            return None;
        }
        let payload = payload(result)?;
        if fault(payload) {
            return None;
        }
        let raw = payload.get("sailry_content")?;
        if serde_json::to_vec(raw).ok()?.len() > MAX_BYTES {
            return None;
        }
        let content: Content = serde_json::from_value(raw.clone()).ok()?;
        if !content.valid()
            || ((failed(result) || failed(payload))
                && !content
                    .blocks
                    .iter()
                    .all(|block| matches!(block, Block::Text(_) | Block::Notice { .. })))
        {
            return None;
        }
        // Failed tools may retain literal diagnostics, while structured success content
        // keeps its existing failure fallback. The controller still owns the failure state.
        let mut bytes = 0;
        for block in &content.blocks {
            if let Block::Text(text) = block {
                bytes += text.read(result)?.len();
                if bytes > MAX_BYTES {
                    return None;
                }
            }
        }
        Some(content)
    }
}

fn payload(result: &Value) -> Option<&Value> {
    if result.get("sailry_content").is_some() {
        Some(result)
    } else {
        result.get("output")
    }
}

fn fault(result: &Value) -> bool {
    result.get("error").is_some_and(|error| !error.is_null())
}

fn failed(result: &Value) -> bool {
    result.get("isError").and_then(Value::as_bool) == Some(true)
}

#[cfg(test)]
mod tests;
