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
    /// Exports that may execute without admission and cannot dispatch mutations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_only: Vec<String>,
    /// Optional bounded policy for one admitted Agent turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<Turn>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<Command>,
    /// Node-computed state for the package's Status contributions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Status {
    /// A read-only export returning Status states for the captured scope.
    pub read: String,
    /// Optional durable export receiving Button invocations from any controller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invoke: Option<String>,
    /// Node signals that re-evaluate the read export for a subscribed scope.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<Change>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Package,
    Conversation,
    Repository,
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
        self.read_only.iter().enumerate().all(|(index, handler)| {
            self.handlers.contains(handler) && !self.read_only[..index].contains(handler)
        }) && self.commands.len() <= 32
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
            && self.status.as_ref().is_none_or(|status| {
                self.read_only.contains(&status.read)
                    && status.invoke.as_ref().is_none_or(|handler| {
                        self.handlers.contains(handler) && !self.read_only.contains(handler)
                    })
                    && status.changes.len() <= 3
                    && status
                        .changes
                        .iter()
                        .enumerate()
                        .all(|(index, change)| !status.changes[..index].contains(change))
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
    fn preserves_wire_and_validation() {
        for (scenario, check) in [
            (
                "confines_modules_and_exports",
                (|scenario: &'static str| {
                    let manifest = Manifest {
                        entry: "dev.sailry.platform/host/main.js".into(),
                        resources: vec!["dev.sailry.platform/host/main.js".into()],
                        handlers: vec!["run".into()],
                        read_only: Vec::new(),
                        turn: None,
                        commands: Vec::new(),
                        status: None,
                    };
                    assert!(manifest.valid(), "{scenario}");
                    for path in [
                        "../main.js",
                        "/main.js",
                        "sailry/sdk.js",
                        "dev.sailry.platform/host/data.json",
                    ] {
                        let mut changed = manifest.clone();
                        changed.entry = path.into();
                        changed.resources = vec![path.into()];
                        assert!(!changed.valid(), "{scenario}");
                    }
                    for names in [vec![], vec!["run", "run"], vec!["1run"], vec!["a.b"]] {
                        let mut changed = manifest.clone();
                        changed.handlers = names.into_iter().map(str::to_owned).collect();
                        assert!(!changed.valid(), "{scenario}");
                    }
                }) as fn(&'static str),
            ),
            (
                "commands_and_terminal_callbacks_require_declared_exports",
                (|scenario: &'static str| {
                    let mut manifest = Manifest {
                        entry: "dev.sailry.platform/host/main.js".into(),
                        resources: vec!["dev.sailry.platform/host/main.js".into()],
                        handlers: vec!["run".into()],
                        read_only: Vec::new(),
                        turn: Some(Turn {
                            initialize: "run".into(),
                            before_model: None,
                            after_turn: Some("run".into()),
                        }),
                        commands: vec![Command {
                            name: "task".into(),
                            handler: "run".into(),
                        }],
                        status: None,
                    };
                    assert!(manifest.valid(), "{scenario}");
                    manifest.commands.push(manifest.commands[0].clone());
                    assert!(!manifest.valid(), "{scenario}");
                    manifest.commands.pop();
                    for name in ["/task", "Task", "task name", ""] {
                        manifest.commands[0].name = name.into();
                        assert!(!manifest.valid(), "{scenario}");
                    }
                    manifest.commands[0].name = "task".into();
                    manifest.commands[0].handler = "missing".into();
                    assert!(!manifest.valid(), "{scenario}");
                    manifest.commands[0].handler = "run".into();
                    manifest.turn.as_mut().unwrap().after_turn = Some("missing".into());
                    assert!(!manifest.valid(), "{scenario}");
                }),
            ),
            (
                "read_exports_require_unique_declared_handlers",
                (|scenario: &'static str| {
                    let mut manifest: Manifest = serde_json::from_value(serde_json::json!({
                    "entry":"dev.sailry.platform/main.js", "resources":["dev.sailry.platform/main.js"],
                    "handlers":["read", "write"]
                }))
                .unwrap();
                    assert!(manifest.read_only.is_empty(), "{scenario}");
                    assert!(manifest.status.is_none(), "{scenario}");
                    for handlers in [vec!["read"], vec!["read", "write"]] {
                        manifest.read_only = handlers.into_iter().map(str::to_owned).collect();
                        assert!(manifest.valid(), "{scenario}");
                    }
                    for handlers in [vec!["missing"], vec!["read", "read"]] {
                        manifest.read_only = handlers.into_iter().map(str::to_owned).collect();
                        assert!(!manifest.valid(), "{scenario}");
                    }
                    let read = crate::Command::ReadPluginCallback {
                        handler: "read".into(),
                        input: serde_json::json!({}),
                    };
                    assert!(!read.durable(), "{scenario}");
                    assert!(
                        crate::Command::CallPlugin {
                            handler: "read".into(),
                            input: serde_json::json!({})
                        }
                        .durable(),
                        "{scenario}"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }

    mod status {
        use super::*;

        fn manifest() -> Manifest {
            serde_json::from_value(serde_json::json!({
                "entry": "dev.sailry.platform/main.js",
                "resources": ["dev.sailry.platform/main.js"],
                "handlers": ["read", "invoke"],
                "read_only": ["read"],
                "status": {"read": "read"}
            }))
            .unwrap()
        }

        #[test]
        fn preserves_wire_and_validation() {
            for (scenario, check) in [
                (
                    "exports_respect_read_only_boundary",
                    (|scenario: &'static str| {
                        let mut manifest = manifest();
                        assert!(manifest.valid(), "{scenario}");
                        manifest.status.as_mut().unwrap().invoke = Some("invoke".into());
                        assert!(manifest.valid(), "{scenario}");
                        for read in ["invoke", "missing"] {
                            manifest.status.as_mut().unwrap().read = read.into();
                            assert!(!manifest.valid(), "{scenario}");
                        }
                        manifest.status.as_mut().unwrap().read = "read".into();
                        for invoke in ["read", "missing"] {
                            manifest.status.as_mut().unwrap().invoke = Some(invoke.into());
                            assert!(!manifest.valid(), "{scenario}");
                        }
                    }) as fn(&'static str),
                ),
                (
                    "changes_are_unique_and_bounded",
                    (|scenario: &'static str| {
                        let mut manifest = manifest();
                        for changes in [
                            vec![],
                            vec![Change::Package],
                            vec![Change::Package, Change::Conversation, Change::Repository],
                        ] {
                            manifest.status.as_mut().unwrap().changes = changes;
                            assert!(manifest.valid(), "{scenario}");
                        }
                        for changes in [
                            vec![Change::Conversation, Change::Conversation],
                            vec![
                                Change::Package,
                                Change::Conversation,
                                Change::Repository,
                                Change::Package,
                            ],
                        ] {
                            manifest.status.as_mut().unwrap().changes = changes;
                            assert!(!manifest.valid(), "{scenario}");
                        }
                        assert_eq!(
                            serde_json::to_value([
                                Change::Package,
                                Change::Conversation,
                                Change::Repository
                            ])
                            .unwrap(),
                            serde_json::json!(["package", "conversation", "repository"]),
                            "{scenario}"
                        );
                    }),
                ),
                (
                    "schema_exposes_host_declaration",
                    (|scenario: &'static str| {
                        let schema = serde_json::to_value(schemars::schema_for!(Manifest)).unwrap();
                        assert!(schema["properties"].get("status").is_some(), "{scenario}");
                        let schema = serde_json::to_value(schemars::schema_for!(Status)).unwrap();
                        assert_eq!(schema["properties"]["read"]["type"], "string", "{scenario}");
                        for property in ["invoke", "changes"] {
                            assert!(schema["properties"].get(property).is_some(), "{scenario}");
                        }
                    }),
                ),
            ] {
                eprintln!("case: {scenario}");
                check(scenario);
            }
        }
    }
}
