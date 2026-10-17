//! Plugin lifecycle actions use the existing catalogs and durable package owner.
use super::super::tools::{self, Binding, Definition, decode};
use super::*;
use sailry_protocol::{
    Command, Output, WorkMode,
    plugin::{self, catalog, skills, updates},
};
use serde::Deserialize;

pub(in crate::agent) const NAMES: [&str; 1] = ["manage_plugins"];

struct Management {
    plan: bool,
}

pub(in crate::agent) fn bind(
    ingress: &Arc<Ingress>,
    invocation: &Invocation,
    stop: &CancellationToken,
) -> Vec<ToolRegistration> {
    tools::bind(
        Binding::new(ingress, invocation, stop),
        [Management {
            plan: invocation.turn.config.mode == WorkMode::Plan,
        }],
    )
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    List,
    Search {
        source: catalog::Source,
        #[serde(default)]
        query: String,
        #[serde(default = "first_page")]
        page: u32,
        #[serde(default)]
        force: bool,
    },
    Inspect {
        target: Target,
    },
    CheckUpdates {
        packages: Vec<updates::Request>,
        #[serde(default)]
        force: bool,
    },
    Install {
        name: String,
        selection: Selection,
    },
    Update {
        name: String,
        expected_revision: u64,
        #[serde(default)]
        force: bool,
    },
    Uninstall {
        name: String,
        expected_revision: u64,
    },
}

impl Action {
    fn read_only(&self) -> bool {
        matches!(
            self,
            Self::List | Self::Search { .. } | Self::Inspect { .. } | Self::CheckUpdates { .. }
        )
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Target {
    Installed {
        name: String,
    },
    Catalog {
        source: catalog::Source,
        id: String,
        #[serde(default)]
        bundled: bool,
    },
    Git {
        source: skills::Source,
    },
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Selection {
    Bundled,
    Git {
        source: skills::Resolved,
        #[serde(default)]
        path: String,
    },
}

impl Selection {
    fn command(self, name: String, expected_revision: u64) -> Command {
        match self {
            Self::Bundled => Command::InstallBundledPlugin {
                name,
                expected_revision,
            },
            Self::Git { source, path } => Command::InstallPluginSource {
                source,
                path,
                name,
                expected_revision,
            },
        }
    }
}

fn first_page() -> u32 {
    1
}

fn feature(info: &plugin::Info) -> Result<(), Fault> {
    if info.skill.is_some() || info.mcp_source.is_some() {
        return Err(Fault::invalid(
            "use manage_skills or manage_mcp for standalone packages",
        ));
    }
    Ok(())
}

fn summary(info: &plugin::Info) -> Value {
    json!({"summary":info.summary,"origin":info.origin,"skills":info.skills,"mcp":info.mcp,"issues":info.issues})
}

async fn execute(binding: &Binding, command: Command) -> Result<Output, Fault> {
    binding
        .transport
        .dispatch(sailry_protocol::Request::new(
            binding.transport.target(),
            command,
        ))
        .await?
        .completion
        .await
        .map_err(|_| {
            Fault::new(
                ErrorCode::Unavailable,
                "plugin inventory response is unavailable",
            )
        })?
}

async fn installed(binding: &Binding, name: &str, revision: u64) -> Result<plugin::Info, Fault> {
    let Output::Plugin(info) = execute(binding, Command::ReadPlugin { name: name.into() }).await?
    else {
        return Err(Fault::invalid("expected an installed plugin"));
    };
    feature(&info)?;
    if revision == 0 || info.summary.revision != revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "plugin changed; reload its revision",
        ));
    }
    Ok(info)
}

fn candidate(
    info: &plugin::Info,
    current: &plugin::Info,
    worktree: sailry_protocol::WorktreeId,
) -> Result<Command, Fault> {
    feature(info)?;
    let previous = current
        .summary
        .version
        .as_deref()
        .and_then(|value| semver::Version::parse(value).ok());
    let next = info
        .summary
        .version
        .as_deref()
        .and_then(|value| semver::Version::parse(value).ok());
    if info.summary.name != current.summary.name
        || !previous
            .zip(next)
            .is_some_and(|(old, new)| new.cmp_precedence(&old).is_gt())
    {
        return Err(Fault::invalid(
            "plugin update requires a newer version of the same package",
        ));
    }
    let name = current.summary.name.clone();
    let expected_revision = current.summary.revision;
    Ok(match info.origin.as_ref() {
        Some(plugin::Origin::Bundled) => Command::InstallBundledPlugin {
            name,
            expected_revision,
        },
        Some(plugin::Origin::Online { source, path }) => Command::InstallPluginSource {
            source: source.clone(),
            path: path.clone(),
            name,
            expected_revision,
        },
        Some(plugin::Origin::Worktree {
            worktree: source,
            path,
        }) if *source == worktree => Command::InstallPlugin {
            worktree,
            path: path.clone(),
            name,
            expected_revision,
            expected_digest: Some(info.summary.digest.clone()),
        },
        _ => {
            return Err(Fault::invalid(
                "this plugin requires another explicit source selection",
            ));
        }
    })
}

impl Management {
    fn action(&self, arguments: Value) -> Result<Action, Fault> {
        let action: Action = decode(arguments)?;
        if self.plan && !action.read_only() {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "planning turns cannot change plugins",
            ));
        }
        Ok(action)
    }
}

#[async_trait]
impl Definition for Management {
    fn name(&self) -> &'static str {
        NAMES[0]
    }
    fn description(&self) -> &'static str {
        "Manage feature plugins on this execution Node. Use action=list, search, inspect, check_updates, install, update or uninstall. Search explicitly selects the real official or third_party catalog, not installed-name filtering. Inspect Git sources before installation and use their exact resolved commit/path/name; bundled installation uses the embedded package. Update checks do not install; update explicitly applies only a newer SemVer candidate and preserves existing configuration and enabled choices. Uploaded/directory packages without reusable provenance require a new explicit source selection in the existing package UI. Standalone skills and MCP servers use their own managers. Never provide credentials. Existing turns retain captured resources. Do not replay uncertain mutations. In Code mode this mixed-action tool uses the ordinary confirmation policy for all calls."
    }
    fn parameters(&self) -> Value {
        parameters(self.plan)
    }
    fn read_only(&self) -> bool {
        self.plan
    }
    fn command(&self, _: &Binding, arguments: Value) -> Result<Command, Fault> {
        Ok(match self.action(arguments)? {
            Action::List => Command::ListPlugins,
            Action::Search {
                source,
                query,
                page,
                force,
            } => Command::SearchPluginCatalog {
                source,
                query,
                page,
                force,
            },
            Action::Inspect { target } => match target {
                Target::Installed { name } => Command::ReadPlugin { name },
                Target::Catalog {
                    source,
                    id,
                    bundled,
                } => Command::ReadCatalogPluginInfo {
                    source,
                    id,
                    bundled,
                },
                Target::Git { source } => Command::InspectPluginSource { source },
            },
            Action::CheckUpdates { packages, force } => {
                Command::CheckPluginUpdates { packages, force }
            }
            Action::Install { name, selection } => selection.command(name, 0),
            Action::Uninstall {
                name,
                expected_revision,
            } if expected_revision > 0 => Command::RemovePlugin {
                name,
                expected_revision,
            },
            Action::Update { .. } | Action::Uninstall { .. } => {
                return Err(Fault::invalid(
                    "plugin mutation requires a current installed revision",
                ));
            }
        })
    }
    async fn prepare(&self, binding: &Binding, arguments: Value) -> Result<Command, Fault> {
        match self.action(arguments.clone())? {
            Action::Update {
                name,
                expected_revision,
                force,
            } => {
                let current = installed(binding, &name, expected_revision).await?;
                let Output::PluginUpdate(report) = execute(
                    binding,
                    Command::CheckPluginUpdate {
                        name,
                        expected_revision,
                        force,
                    },
                )
                .await?
                else {
                    return Err(Fault::invalid("expected a plugin update report"));
                };
                if report.current != current.summary {
                    return Err(Fault::new(
                        ErrorCode::RevisionConflict,
                        "plugin changed during inspection",
                    ));
                }
                let info = match report.available {
                    Some(updates::Candidate::Package(info)) => *info,
                    Some(updates::Candidate::Official(entry)) => {
                        let Output::Plugin(info) = execute(
                            binding,
                            Command::ReadCatalogPluginInfo {
                                source: catalog::Source::Official,
                                id: entry.id,
                                bundled: false,
                            },
                        )
                        .await?
                        else {
                            return Err(Fault::invalid("expected an official plugin selection"));
                        };
                        if info.summary.version != entry.version {
                            return Err(Fault::new(
                                ErrorCode::RevisionConflict,
                                "plugin catalog changed during inspection",
                            ));
                        }
                        info
                    }
                    None => {
                        return Err(Fault::new(
                            ErrorCode::NotFound,
                            "no newer plugin version is available",
                        ));
                    }
                };
                candidate(&info, &current, binding.worktree)
            }
            Action::Uninstall {
                ref name,
                expected_revision,
            } => {
                installed(binding, name, expected_revision).await?;
                self.command(binding, arguments)
            }
            _ => self.command(binding, arguments),
        }
    }
    async fn output(&self, binding: &Binding, output: Output) -> Result<Value, Fault> {
        match output {
            Output::Plugins(entries) => {
                let mut packages = Vec::new();
                for entry in entries {
                    if let Output::Plugin(info) =
                        execute(binding, Command::ReadPlugin { name: entry.name }).await?
                    {
                        if feature(&info).is_ok() {
                            packages.push(summary(&info));
                        }
                    }
                }
                Ok(json!({"plugins":packages}))
            }
            Output::Plugin(info) => {
                feature(&info)?;
                Ok(summary(&info))
            }
            Output::PluginSource(selection) => {
                feature(&selection.info)?;
                Ok(
                    json!({"source":selection.source,"path":selection.path,"plugin":summary(&selection.info)}),
                )
            }
            Output::PluginCatalog(page) => Ok(json!({"catalog":page})),
            Output::PluginUpdates(batch) => {
                let reports: Vec<_> = batch.reports.iter().map(|report| {
                    let available = report.available.as_ref().map(|candidate| match candidate {
                        updates::Candidate::Package(info) => summary(info),
                        updates::Candidate::Official(entry) => json!({"catalog":entry,"source":"official"}),
                    });
                    json!({"current":report.current,"available":available,"manual":report.manual})
                }).collect();
                Ok(json!({"updates":reports,"failures":batch.failures}))
            }
            _ => Err(Fault::invalid("unexpected plugin management result")),
        }
    }
}

fn parameters(plan: bool) -> Value {
    let name = json!({"type":"string","minLength":1});
    let revision = json!({"type":"integer","minimum":1});
    let catalog = json!({"enum":["official","third_party"]});
    let resolved = json!({"type":"object","additionalProperties":false,"required":["repository","git_ref","commit"],"properties":{"repository":{"type":"string"},"git_ref":{"type":"string"},"commit":{"type":"string","pattern":"^[0-9a-f]{40}$"}}});
    let git = json!({"type":"object","additionalProperties":false,"required":["repository"],"properties":{"repository":{"type":"string"},"git_ref":{"type":["string","null"]},"path":{"type":["string","null"]}}});
    let object = |action: &str, fields: Value, required: Vec<&str>| {
        let mut properties = fields.as_object().unwrap().clone();
        properties.insert("action".into(), json!({"const":action}));
        let mut required = required;
        required.push("action");
        json!({"type":"object","additionalProperties":false,"properties":properties,"required":required})
    };
    let mut variants = vec![
        object("list", json!({}), vec![]),
        object(
            "search",
            json!({"source":catalog,"query":{"type":"string"},"page":{"type":"integer","minimum":1,"default":1},"force":{"type":"boolean"}}),
            vec!["source"],
        ),
        object(
            "inspect",
            json!({"target":{"oneOf":[
                {"type":"object","additionalProperties":false,"required":["kind","name"],"properties":{"kind":{"const":"installed"},"name":name}},
                {"type":"object","additionalProperties":false,"required":["kind","source","id"],"properties":{"kind":{"const":"catalog"},"source":catalog,"id":name,"bundled":{"type":"boolean"}}},
                {"type":"object","additionalProperties":false,"required":["kind","source"],"properties":{"kind":{"const":"git"},"source":git}}
            ]}}),
            vec!["target"],
        ),
        object(
            "check_updates",
            json!({"packages":{"type":"array","maxItems":plugin::MAX_INSTALLED,"items":{"type":"object","additionalProperties":false,"required":["name","expected_revision"],"properties":{"name":name,"expected_revision":revision}}},"force":{"type":"boolean"}}),
            vec!["packages"],
        ),
    ];
    if !plan {
        variants.extend([
            object("install",json!({"name":name,"selection":{"oneOf":[
                {"type":"object","additionalProperties":false,"required":["kind"],"properties":{"kind":{"const":"bundled"}}},
                {"type":"object","additionalProperties":false,"required":["kind","source"],"properties":{"kind":{"const":"git"},"source":resolved,"path":{"type":"string"}}}
            ]}}),vec!["name","selection"]),
            object("update",json!({"name":name,"expected_revision":revision,"force":{"type":"boolean"}}),vec!["name","expected_revision"]),
            object("uninstall",json!({"name":name,"expected_revision":revision}),vec!["name","expected_revision"]),
        ]);
    }
    let mut properties = serde_json::Map::new();
    let mut actions = Vec::new();
    for variant in &variants {
        let fields = variant["properties"].as_object().expect("action schema");
        for (name, schema) in fields {
            if name == "action" {
                actions.push(schema["const"].clone());
            } else {
                properties.insert(name.clone(), schema.clone());
            }
        }
    }
    properties.insert("action".into(), json!({"type":"string","enum":actions}));
    json!({"type":"object","additionalProperties":false,"properties":properties,"required":["action"],"oneOf":variants})
}

#[cfg(test)]
mod tests;
