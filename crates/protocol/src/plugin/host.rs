//! Node-only JavaScript entry points, independent of desktop rendering.
use serde::{Deserialize, Serialize};

pub const MAX_BYTES: usize = 256 * 1024;
/// Includes bounded multi-value transactions and JSON escaping in their envelope.
pub const MAX_DATA_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[schemars(transform = super::schema::resource_path)]
    pub entry: String,
    #[schemars(transform = super::schema::resource_paths)]
    pub resources: Vec<String>,
    /// Named module exports callable through ordinary durable admission.
    pub handlers: Vec<String>,
    /// Optional bounded policy for one admitted Agent turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<Turn>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<Command>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub name: String,
    pub handler: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    pub initialize: String,
    pub before_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_turn: Option<String>,
}

impl Manifest {
    pub fn valid(&self) -> bool {
        self.commands.len() <= 32
            && self.commands.iter().enumerate().all(|(index, command)| {
                super::ui::command_name(&command.name)
                    && self.handlers.contains(&command.handler)
                    && !self.commands[..index]
                        .iter()
                        .any(|entry| entry.name == command.name)
            })
            && self.turn.as_ref().is_none_or(|turn| {
                self.handlers.contains(&turn.initialize)
                    && turn
                        .before_model
                        .as_ref()
                        .is_none_or(|name| self.handlers.contains(name))
                    && turn
                        .after_turn
                        .as_ref()
                        .is_none_or(|name| self.handlers.contains(name))
            })
            && self.resources.contains(&self.entry)
            && !self.resources.is_empty()
            && self.resources.len() <= 64
            && super::desktop::valid_paths(self.resources.iter().map(String::as_str))
            && self.resources.iter().all(|path| {
                (path.ends_with(".js") || path.ends_with(".mjs")) && !path.starts_with("sailry/")
            })
            && !self.handlers.is_empty()
            && self.handlers.len() <= 64
            && self
                .handlers
                .iter()
                .enumerate()
                .all(|(index, name)| valid_handler(name) && !self.handlers[..index].contains(name))
    }
}

pub fn valid_handler(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphabetic() || byte == b'_' || (index > 0 && byte.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confines_modules_and_exports() {
        let manifest = Manifest {
            entry: "dev.sailry.platform/host/main.js".into(),
            resources: vec!["dev.sailry.platform/host/main.js".into()],
            handlers: vec!["run".into()],
            turn: None,
            commands: Vec::new(),
        };
        assert!(manifest.valid());
        for path in [
            "../main.js",
            "/main.js",
            "sailry/sdk.js",
            "dev.sailry.platform/host/data.json",
        ] {
            let mut changed = manifest.clone();
            changed.entry = path.into();
            changed.resources = vec![path.into()];
            assert!(!changed.valid());
        }
        for names in [vec![], vec!["run", "run"], vec!["1run"], vec!["a.b"]] {
            let mut changed = manifest.clone();
            changed.handlers = names.into_iter().map(str::to_owned).collect();
            assert!(!changed.valid());
        }
    }

    #[test]
    fn commands_and_terminal_callbacks_require_declared_exports() {
        let mut manifest = Manifest {
            entry: "dev.sailry.platform/host/main.js".into(),
            resources: vec!["dev.sailry.platform/host/main.js".into()],
            handlers: vec!["run".into()],
            turn: Some(Turn {
                initialize: "run".into(),
                before_model: None,
                after_turn: Some("run".into()),
            }),
            commands: vec![Command {
                name: "task".into(),
                handler: "run".into(),
            }],
        };
        assert!(manifest.valid());
        manifest.commands.push(manifest.commands[0].clone());
        assert!(!manifest.valid());
        manifest.commands.pop();
        for name in ["/task", "Task", "task name", ""] {
            manifest.commands[0].name = name.into();
            assert!(!manifest.valid());
        }
        manifest.commands[0].name = "task".into();
        manifest.commands[0].handler = "missing".into();
        assert!(!manifest.valid());
        manifest.commands[0].handler = "run".into();
        manifest.turn.as_mut().unwrap().after_turn = Some("missing".into());
        assert!(!manifest.valid());
    }
}
