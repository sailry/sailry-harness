use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    Codex,
    Claude,
    Gemini,
    Agy,
    Grok,
    Opencode,
    Kimi,
}

impl Tool {
    pub const ALL: [Self; 7] = [
        Self::Codex,
        Self::Claude,
        Self::Gemini,
        Self::Agy,
        Self::Grok,
        Self::Opencode,
        Self::Kimi,
    ];

    pub const fn executable(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Gemini => "gemini",
            Self::Agy => "agy",
            Self::Grok => "grok",
            Self::Opencode => "opencode",
            Self::Kimi => "kimi",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ToolInfo {
    pub tool: Tool,
    pub available: bool,
}
