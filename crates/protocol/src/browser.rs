//! Controller browser calls are transient and are never replayed after reconnect.
use crate::{Fault, RequestId, SessionId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Tabs,
    Read {
        tab: Option<u64>,
    },
    Navigate {
        url: String,
        tab: Option<u64>,
    },
    Click {
        tab: u64,
        snapshot: String,
        element: u64,
    },
    Input {
        tab: u64,
        snapshot: String,
        element: u64,
        text: String,
    },
    Scroll {
        tab: u64,
        direction: Direction,
    },
    Back {
        tab: u64,
    },
    Forward {
        tab: u64,
    },
    Refresh {
        tab: u64,
    },
    Open {
        url: String,
    },
    Close {
        tab: u64,
    },
    Focus {
        tab: u64,
    },
    Select {
        tab: u64,
        snapshot: String,
        element: u64,
        value: String,
    },
    Hover {
        tab: u64,
        snapshot: String,
        element: u64,
    },
    Key {
        tab: u64,
        snapshot: String,
        element: u64,
        key: Key,
    },
    Frame {
        tab: u64,
        snapshot: String,
        element: Option<u64>,
    },
    Wait {
        tab: u64,
        condition: Condition,
        timeout_ms: u32,
    },
    Screenshot {
        tab: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Key {
    Enter,
    Escape,
    Tab,
    Backspace,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Condition {
    Text { text: String },
    Visible { selector: String },
    Hidden { selector: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Up,
    Down,
}

impl Action {
    /// Approval policy includes artifact writes independently of response uncertainty.
    pub fn requires_approval(&self) -> bool {
        !matches!(
            self,
            Self::Tabs
                | Self::Read { .. }
                | Self::Navigate { .. }
                | Self::Scroll { .. }
                | Self::Back { .. }
                | Self::Forward { .. }
                | Self::Refresh { .. }
                | Self::Open { .. }
                | Self::Focus { .. }
                | Self::Frame { .. }
                | Self::Wait { .. }
        )
    }

    /// Missing responses for these actions do not imply uncertain page changes.
    pub fn read_only(&self) -> bool {
        matches!(
            self,
            Self::Tabs | Self::Read { .. } | Self::Wait { .. } | Self::Screenshot { .. }
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    pub id: RequestId,
    pub session: SessionId,
    pub action: Action,
    pub expires_at_ms: u64,
}

pub type Result = std::result::Result<serde_json::Value, Fault>;
