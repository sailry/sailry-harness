// Adapted from sailry-code 67ae9fa0, terminal_workspace.rs. See third_party_licenses/sailry-code-terminal.md.
mod features;
mod graphics;
mod input;
pub use features::*;
pub use graphics::*;
mod screen;
mod settings;
mod tools;
use crate::{NodeId, TerminalId, WorktreeId};
pub use input::*;
pub use screen::*;
use serde::{Deserialize, Serialize};
pub use settings::{Settings, validate_key};
pub use tools::{Tool, ToolInfo};

pub const MAX_COLUMNS: u16 = 500;
pub const MAX_ROWS: u16 = 200;
pub const MAX_SCROLLBACK_ROWS: usize = 1000;
pub const MAX_INPUT_BYTES: usize = 64 * 1024;
pub const MAX_KEY_UTF8_BYTES: usize = 64;
pub const FUNCTION_KEY_COUNT: usize = 25;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Viewport {
    pub columns: u16,
    pub rows: u16,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorScheme {
    Light,
    Dark,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Appearance {
    pub foreground: Rgb,
    pub background: Rgb,
    pub palette: [Rgb; 16],
    pub color_scheme: ColorScheme,
}

impl Viewport {
    pub fn validate(&self) -> Result<(), crate::Fault> {
        if !(1..=MAX_COLUMNS).contains(&self.columns) || !(1..=MAX_ROWS).contains(&self.rows) {
            return Err(crate::Fault::new(
                crate::ErrorCode::InvalidRequest,
                "terminal viewport is outside the supported range",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Launch {
    pub worktree: WorktreeId,
    pub viewport: Viewport,
    pub appearance: Appearance,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Status {
    Running,
    Exited { code: u32 },
    Closed,
    Stopped,
    Failed { message: String },
}

/// Reported terminal work, independent of whether its shell process is alive.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    #[default]
    Idle,
    Working,
    WaitingForInput,
    Completed,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ActivityReport {
    pub state: Activity,
    pub sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Info {
    pub id: TerminalId,
    pub worktree: Option<WorktreeId>,
    pub ssh: Option<crate::SshId>,
    pub tool: Option<Tool>,
    pub status: Status,
    /// Absent until the terminal reports activity through OSC.
    pub activity: Option<ActivityReport>,
    pub title: Option<String>,
    pub directory: Option<String>,
    pub owner: Option<NodeId>,
    pub revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Snapshot {
    pub node: NodeId,
    pub info: Info,
    pub sequence: u64,
    pub screen: Screen,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Frame {
    pub node: NodeId,
    pub info: Info,
    pub sequence: u64,
    pub screen: ScreenUpdate,
}
