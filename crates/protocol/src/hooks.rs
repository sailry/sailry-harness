//! Native-controller hook configuration; scripts execute only on the owning Node.
use serde::{Deserialize, Serialize};

fn enabled() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    #[serde(default)]
    pub name: String,
    #[serde(default = "enabled")]
    pub enabled: bool,
    pub command: String,
    pub timeout_ms: u64,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            name: String::new(),
            enabled: true,
            command: String::new(),
            timeout_ms: 60_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub phase: Phase,
    pub script: Script,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub revision: u64,
    pub enabled: bool,
    pub before_tool: Option<Script>,
    pub after_tool: Option<Script>,
    pub turn_end: Option<Script>,
    #[serde(default)]
    pub additional: Vec<Entry>,
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
    pub const ALL: [Self; 3] = [Self::BeforeTool, Self::AfterTool, Self::TurnEnd];

    pub fn name(self) -> &'static str {
        match self {
            Self::BeforeTool => "before_tool",
            Self::AfterTool => "after_tool",
            Self::TurnEnd => "turn_end",
        }
    }
}

impl Settings {
    pub fn slot(&self, phase: Phase) -> Option<&Script> {
        match phase {
            Phase::BeforeTool => self.before_tool.as_ref(),
            Phase::AfterTool => self.after_tool.as_ref(),
            Phase::TurnEnd => self.turn_end.as_ref(),
        }
    }

    pub fn entries(&self) -> impl Iterator<Item = (&str, Phase, &Script)> {
        Phase::ALL
            .into_iter()
            .filter_map(|phase| self.slot(phase).map(|script| (phase.name(), phase, script)))
            .chain(
                self.additional
                    .iter()
                    .map(|entry| (entry.id.as_str(), entry.phase, &entry.script)),
            )
    }

    pub fn scripts(&self, phase: Phase) -> impl Iterator<Item = (&str, &Script)> {
        self.entries().filter_map(move |(id, current, script)| {
            (self.enabled && current == phase && script.enabled).then_some((id, script))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_missing_fields_without_reordering() {
        let settings: Settings = serde_json::from_value(serde_json::json!({"revision":2,"enabled":true,"before_tool":{"command":"first","timeout_ms":100},"after_tool":null,"turn_end":null})).unwrap();
        assert!(settings.additional.is_empty());
        assert!(settings.before_tool.as_ref().unwrap().enabled);
        assert!(settings.before_tool.as_ref().unwrap().name.is_empty());
        let mut settings = settings;
        settings.additional = vec![
            Entry {
                id: "second".into(),
                phase: Phase::BeforeTool,
                script: Script {
                    command: "second".into(),
                    ..Default::default()
                },
            },
            Entry {
                id: "disabled".into(),
                phase: Phase::BeforeTool,
                script: Script {
                    enabled: false,
                    command: "disabled".into(),
                    ..Default::default()
                },
            },
        ];
        assert_eq!(
            settings
                .scripts(Phase::BeforeTool)
                .map(|(id, _)| id)
                .collect::<Vec<_>>(),
            ["before_tool", "second"]
        );
        assert_eq!(
            serde_json::from_value::<Settings>(serde_json::to_value(&settings).unwrap()).unwrap(),
            settings
        );
    }
}
