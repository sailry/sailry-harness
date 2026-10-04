//! UI contributions describe product slots, never an arbitrary layout tree.
use super::desktop::Navigation;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(test)]
mod tests;

pub const MAX_ENTRIES: usize = 32;
pub const MAX_CHOICES: usize = 256;
pub const MAX_STATE_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    Composer,
    Context,
    Statistics,
    Status,
    Project,
    ProjectMenu,
    Commands,
}

impl Slot {
    pub fn is_project(self) -> bool {
        matches!(self, Self::Project | Self::ProjectMenu)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Button,
    Popover,
    Toggle,
    Select,
    Menu,
    Picker,
    Metric,
    Indicator,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Overflow {
    #[default]
    Auto,
    Menu,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Start,
    End,
}

/// Product entry points resolve an enabled declaration rather than a package name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    GitBranches,
    Worktrees,
    CreateWorktree,
    ForkWorktree,
}

impl AsRef<str> for Intent {
    fn as_ref(&self) -> &str {
        match self {
            Self::GitBranches => "git_branches",
            Self::Worktrees => "worktrees",
            Self::CreateWorktree => "create_worktree",
            Self::ForkWorktree => "fork_worktree",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct Contribution {
    pub intent: Option<String>,
    pub id: String,
    pub slot: Slot,
    pub kind: Kind,
    pub label: Navigation,
    pub icon: Option<String>,
    pub handler: Option<String>,
    #[serde(default)]
    pub order: i16,
    #[serde(default)]
    pub overflow: Overflow,
    #[serde(default)]
    pub align: Align,
    #[serde(default)]
    pub choices: Vec<Choice>,
    /// A command and its ordinary control share one captured state and handler.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Command>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct Command {
    pub name: String,
    pub kind: CommandKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    Invoke,
    Message,
}

pub fn command_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct Choice {
    pub id: String,
    pub label: Navigation,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub description: Option<Navigation>,
    pub icon: Option<String>,
    pub group: Option<Navigation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detail {
    pub label: Navigation,
    pub value: String,
}

/// One state serves the toolbar, the compact menu and standard search overlays.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub id: String,
    pub label: Option<Navigation>,
    pub icon: Option<String>,
    #[serde(default)]
    pub dropdown: bool,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub value: Value,
    #[serde(default)]
    pub choices: Option<Vec<Choice>>,
    #[serde(default)]
    pub details: Vec<Detail>,
    #[serde(default)]
    pub segments: Vec<Segment>,
    /// Responding to a search/click cannot replace a newer control interaction.
    pub reply_to: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Invoke,
    Change,
    Search,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub sequence: u64,
    pub id: String,
    pub handler: String,
    pub kind: EventKind,
    pub value: Value,
}

fn yes() -> bool {
    true
}

pub fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || b"._-".contains(&ch))
}

pub fn icon(value: &Option<String>) -> bool {
    value.as_ref().is_none_or(|value| {
        (identifier(value) && !value.contains('.'))
            || super::desktop::Icon::Name(value.clone()).valid()
    })
}

impl Contribution {
    pub fn valid(&self) -> bool {
        identifier(&self.id)
            && self.intent.as_deref().is_none_or(identifier)
            && (self.intent.is_none() || matches!(self.kind, Kind::Button | Kind::Popover))
            && (self.kind != Kind::Popover
                || matches!(self.slot, Slot::Composer | Slot::Context | Slot::Status))
            && self.label.valid()
            && icon(&self.icon)
            && (self.slot != Slot::Status || matches!(self.kind, Kind::Button | Kind::Popover))
            && (self.slot != Slot::Commands || self.command.is_some())
            && self.command.as_ref().is_none_or(|command| {
                command_name(&command.name)
                    && ![
                        "model",
                        "reasoning",
                        "plan",
                        "code",
                        "compact",
                        "review",
                        "test",
                        "explain",
                    ]
                    .contains(&command.name.as_str())
                    && matches!(
                        self.slot,
                        Slot::Composer | Slot::Context | Slot::Status | Slot::Commands
                    )
                    && match command.kind {
                        CommandKind::Message => {
                            self.kind == Kind::Button && self.slot == Slot::Commands
                        }
                        CommandKind::Invoke => matches!(
                            self.kind,
                            Kind::Button | Kind::Toggle | Kind::Popover | Kind::Picker
                        ),
                    }
            })
            && (self.slot != Slot::ProjectMenu || matches!(self.kind, Kind::Button | Kind::Menu))
            && if self.kind == Kind::Metric {
                self.slot == Slot::Statistics && self.handler.is_none() && self.choices.is_empty()
            } else if self.kind == Kind::Indicator {
                self.slot == Slot::Composer && self.handler.is_none() && self.choices.is_empty()
            } else {
                self.slot != Slot::Statistics
                    && self.handler.as_ref().is_some_and(|name| identifier(name))
                    && choices(&self.choices)
                    && (matches!(self.kind, Kind::Select | Kind::Picker | Kind::Menu)
                        || self.choices.is_empty())
            }
    }
}

pub fn valid(entries: &[Contribution]) -> bool {
    entries.len() <= MAX_ENTRIES
        && entries
            .iter()
            .filter(|entry| entry.kind == Kind::Popover)
            .count()
            <= 1
        && entries.iter().enumerate().all(|(index, entry)| {
            entry.valid()
                && !entries[..index].iter().any(|before| {
                    before.id == entry.id
                        || entry.command.as_ref().is_some_and(|command| {
                            before
                                .command
                                .as_ref()
                                .is_some_and(|other| other.name == command.name)
                        })
                })
        })
}

fn choices(entries: &[Choice]) -> bool {
    entries.len() <= MAX_CHOICES
        && entries.iter().enumerate().all(|(index, entry)| {
            !entry.id.is_empty()
                && entry.id.len() <= 512
                && !entry.id.chars().any(char::is_control)
                && entry.label.valid()
                && icon(&entry.icon)
                && entry.description.as_ref().is_none_or(Navigation::valid)
                && entry.group.as_ref().is_none_or(Navigation::valid)
                && !entries[..index].iter().any(|before| before.id == entry.id)
        })
}

impl State {
    pub fn pending(entry: &Contribution) -> Self {
        Self {
            id: entry.id.clone(),
            label: None,
            icon: None,
            dropdown: false,
            visible: true,
            enabled: false,
            value: Value::Null,
            choices: None,
            details: vec![],
            segments: vec![],
            reply_to: None,
        }
    }

    pub fn valid(&self, entry: &Contribution) -> bool {
        self.id == entry.id
            && self.label.as_ref().is_none_or(Navigation::valid)
            && icon(&self.icon)
            && (!self.dropdown || matches!(entry.kind, Kind::Button | Kind::Popover))
            && self.choices.as_ref().is_none_or(|entries| choices(entries))
            && self.segments.len() <= 16
            && (self.segments.is_empty() || entry.slot == Slot::Status)
            && self.segments.iter().all(|segment| segment.label.valid())
            && self.details.len() <= MAX_ENTRIES
            && self
                .details
                .iter()
                .all(|row| row.label.valid() && row.value.len() <= 4096)
            && !self.value.is_array()
            && (if entry.kind == Kind::Indicator {
                self.value.is_null()
                    || serde_json::from_value::<Indicator>(self.value.clone())
                        .is_ok_and(|value| value.valid())
            } else {
                !self.value.is_object()
            })
            && self.value.as_str().is_none_or(|text| text.len() <= 4096)
            && (!matches!(entry.kind, Kind::Toggle | Kind::Popover)
                || self.value.is_boolean()
                || self.value.is_null())
            && (matches!(entry.kind, Kind::Select | Kind::Picker | Kind::Menu)
                || self.choices.is_none())
            && (entry.kind == Kind::Metric || self.details.is_empty())
    }
}

/// Finite presentation state for the native Kit progress indicator.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Indicator {
    pub percent: Option<f64>,
    #[serde(default)]
    pub loading: bool,
    pub tone: Tone,
    pub hint: String,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    Muted,
    Warning,
    Danger,
}
impl Indicator {
    fn valid(&self) -> bool {
        self.percent
            .is_none_or(|value| value.is_finite() && value >= 0.)
            && self.hint.len() <= 4096
    }
}

/// Short labeled values rendered with the host's semantic status colors.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub label: Navigation,
    pub tone: SegmentTone,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentTone {
    Muted,
    Success,
    Warning,
    Danger,
}
