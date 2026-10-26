use super::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStream {
    #[default]
    Stdout,
    Stderr,
}

/// Text retained by the existing command owner, independently of UI captures.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutputPage {
    pub info: Info,
    pub stream: OutputStream,
    pub offset: usize,
    pub total_bytes: u64,
    pub next_offset: Option<usize>,
    pub text: String,
    /// Some observed bytes are unavailable from the retained output.
    pub truncated: bool,
    pub invalid_utf8: bool,
}
