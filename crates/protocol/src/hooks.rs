//! Native-controller hook configuration; scripts execute only on the owning Node.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    pub command: String,
    pub timeout_ms: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub revision: u64,
    pub enabled: bool,
    pub before_tool: Option<Script>,
    pub after_tool: Option<Script>,
    pub turn_end: Option<Script>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub worktree: crate::WorktreeId,
    pub settings: Settings,
    pub sandboxed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    BeforeTool,
    AfterTool,
    TurnEnd,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Self::BeforeTool => "before_tool",
            Self::AfterTool => "after_tool",
            Self::TurnEnd => "turn_end",
        }
    }
}

impl Settings {
    pub fn script(&self, phase: Phase) -> Option<&Script> {
        if !self.enabled {
            return None;
        }
        match phase {
            Phase::BeforeTool => self.before_tool.as_ref(),
            Phase::AfterTool => self.after_tool.as_ref(),
            Phase::TurnEnd => self.turn_end.as_ref(),
        }
    }
}
