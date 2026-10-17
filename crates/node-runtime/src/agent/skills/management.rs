//! Standalone Skill management reuses the Node's inventory and durable package commands.
use super::super::tools::{Binding, Definition, decode};
use super::*;
use sailry_protocol::{
    Command, Output,
    plugin::{
        self,
        skills::{Imported, Provenance, RegistryResolved, RegistrySource, Resolved, Source},
    },
};

pub(in crate::agent) const NAMES: [&str; 1] = ["manage_skills"];

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Operation {
    List,
    Discover,
    Install,
    Update,
    Uninstall,
}

pub(in crate::agent) fn bind(
    ingress: &Arc<Ingress>,
    invocation: &Invocation,
    stop: &CancellationToken,
) -> Vec<catalog::Registration> {
    tools::bind(
        Binding::new(ingress, invocation, stop),
        [Management {
            read_only: invocation.turn.config.mode == sailry_protocol::WorkMode::Plan,
        }],
    )
}

struct Management {
    read_only: bool,
}

impl Management {
    fn arguments(&self, mut value: Value) -> Result<(Operation, Value), Fault> {
        let action = value
            .as_object_mut()
            .and_then(|value| value.remove("action"))
            .ok_or_else(|| Fault::invalid("select a skill management action"))?;
        let action: Operation = decode(action)?;
        if self.read_only && !action.read_only() {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "skill installation changes are unavailable in Plan mode",
            ));
        }
        Ok((action, value))
    }
}

#[async_trait]
impl Definition for Management {
    fn name(&self) -> &'static str {
        NAMES[0]
    }
    fn description(&self) -> &'static str {
        "Manage standalone skills on this execution Node. Actions: list, discover, install, update, uninstall. Discover accepts Git/skills.sh/Skillhub detail links, registry coordinates, or an execution-Node relative directory/archive path. Discovery does not install or execute source content. Use its exact source, name/path and import stream; update/uninstall require the current installed revision. Registry updates require a newer semantic version, imports require rediscovery. Changes affect new selections, never this turn's frozen resources. Never supply credentials, substitute a branch for a commit, or replay uncertain writes. Plan permits list/discover only."
    }
    fn parameters(&self) -> Value {
        let actions = [
            Operation::List,
            Operation::Discover,
            Operation::Install,
            Operation::Update,
            Operation::Uninstall,
        ];
        let variants = actions
            .into_iter()
            .filter(|action| !self.read_only || action.read_only())
            .map(|action| {
                let mut schema = action.parameters();
                let name = match action {
                    Operation::List => "list",
                    Operation::Discover => "discover",
                    Operation::Install => "install",
                    Operation::Update => "update",
                    Operation::Uninstall => "uninstall",
                };
                schema["properties"]["action"] = json!({"const":name});
                schema
                    .as_object_mut()
                    .unwrap()
                    .entry("required")
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                    .unwrap()
                    .push(json!("action"));
                schema
            })
            .collect::<Vec<_>>();
        let mut properties = serde_json::Map::new();
        for variant in &variants {
            properties.extend(variant["properties"].as_object().unwrap().clone());
        }
        properties.insert("action".into(),json!({"type":"string","enum":variants.iter().map(|variant|variant["properties"]["action"]["const"].clone()).collect::<Vec<_>>()}));
        json!({"type":"object","properties":properties,"required":["action"],"additionalProperties":false,"oneOf":variants})
    }
    fn read_only(&self) -> bool {
        self.read_only
    }
    fn command(&self, binding: &Binding, arguments: Value) -> Result<Command, Fault> {
        let (action, arguments) = self.arguments(arguments)?;
        action.command(binding, arguments)
    }
    async fn prepare(&self, binding: &Binding, arguments: Value) -> Result<Command, Fault> {
        let (action, arguments) = self.arguments(arguments)?;
        action.prepare(binding, arguments).await
    }
    async fn output(&self, binding: &Binding, output: Output) -> Result<Value, Fault> {
        // List and removal both return the remaining canonical skill inventory.
        Operation::List.output(binding, output).await
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Deserialize)]
#[serde(untagged)]
enum Discover {
    Git(GitDiscovery),
    Registry(RegistryDiscovery),
    Local(LocalDiscovery),
}
impl Discover {
    fn command(self, worktree: sailry_protocol::WorktreeId) -> Result<Command, Fault> {
        Ok(match self {
            Self::Git(args) => {
                if let Some(source) = crate::plugins::registry::source_url(&args.repository)? {
                    if args.git_ref.is_some() || args.path.is_some() {
                        return Err(Fault::invalid(
                            "registry links do not take a Git ref or path",
                        ));
                    }
                    Command::DiscoverRegistrySkill { source }
                } else {
                    Command::DiscoverSkills {
                        source: Source {
                            repository: args.repository,
                            git_ref: args.git_ref,
                            path: args.path,
                        },
                    }
                }
            }
            Self::Local(args) => Command::DiscoverLocalSkills {
                worktree,
                path: args.path,
            },
            Self::Registry(args) => Command::DiscoverRegistrySkill {
                source: args.registry,
            },
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GitDiscovery {
    repository: String,
    #[serde(rename = "ref")]
    git_ref: Option<String>,
    path: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryDiscovery {
    registry: RegistrySource,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalDiscovery {
    path: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum InstallSource {
    Git(Resolved),
    Registry(RegistryResolved),
    Imported(Imported),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Install {
    source: InstallSource,
    stream: Option<sailry_protocol::StreamId>,
    #[serde(default)]
    path: String,
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Update {
    source: InstallSource,
    stream: Option<sailry_protocol::StreamId>,
    #[serde(default)]
    path: String,
    name: String,
    expected_revision: u64,
}

impl InstallSource {
    fn command(
        self,
        path: String,
        name: String,
        expected_revision: u64,
        stream: Option<sailry_protocol::StreamId>,
    ) -> Result<Command, Fault> {
        if stream.is_some() && !matches!(self, Self::Imported(_)) {
            return Err(Fault::invalid("only imported skills use an upload stream"));
        }
        Ok(match self {
            Self::Git(source) => Command::InstallSkill {
                source,
                path,
                name,
                expected_revision,
            },
            Self::Imported(source) => {
                if !path.is_empty() && path != source.path {
                    return Err(Fault::invalid(
                        "skill path does not match the imported directory",
                    ));
                }
                Command::InstallSkillUpload {
                    source,
                    name,
                    expected_revision,
                    stream: stream.ok_or_else(|| {
                        Fault::invalid(
                            "rediscover the imported skill to obtain its current upload stream",
                        )
                    })?,
                }
            }
            Self::Registry(source) => {
                if !path.is_empty() {
                    return Err(Fault::invalid(
                        "registry skills do not have a Git directory",
                    ));
                }
                Command::InstallRegistrySkill {
                    source,
                    name,
                    expected_revision,
                }
            }
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Remove {
    name: String,
    expected_revision: u64,
}

async fn execute(binding: &Binding, command: Command) -> Result<Output, Fault> {
    let admission = binding
        .transport
        .dispatch(sailry_protocol::Request::new(
            binding.transport.target(),
            command,
        ))
        .await?;
    admission.completion.await.map_err(|_| {
        Fault::new(
            ErrorCode::Unavailable,
            "skill inventory response is unavailable",
        )
    })?
}

async fn standalone(
    binding: &Binding,
    name: &str,
    expected_revision: u64,
) -> Result<plugin::Info, Fault> {
    let Output::Plugin(info) = execute(binding, Command::ReadPlugin { name: name.into() }).await?
    else {
        return Err(sailry_protocol::Fault::invalid("expected a skill package"));
    };
    if info.skill.is_none() {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "only standalone skill packages can be managed by this tool",
        ));
    }
    if info.summary.revision != expected_revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "skill changed; reload its revision",
        ));
    }
    Ok(info)
}

#[async_trait]
impl Definition for Operation {
    fn name(&self) -> &'static str {
        NAMES[0]
    }
    fn description(&self) -> &'static str {
        Management { read_only: false }.description()
    }
    fn parameters(&self) -> Value {
        let source = json!({"type":"object","additionalProperties":false,"required":["repository","git_ref","commit"],"properties":{
            "repository":{"type":"string","description":"Canonical repository from discovery, without credentials"},
            "git_ref":{"type":"string","description":"Git ref from discovery"},
            "commit":{"type":"string","pattern":"^[0-9a-f]{40}$","description":"Exact resolved commit from discovery"}
        }});
        let registry = json!({"type":"object","additionalProperties":false,"required":["namespace","slug","version","version_id","content_hash","package_md5","archive_sha256"],"properties":{
            "namespace":{"type":"string"},"slug":{"type":"string"},"version":{"type":"string"},"version_id":{"type":"string"},"content_hash":{"type":"string"},"package_md5":{"type":"string"},"archive_sha256":{"type":"string"}
        }});
        let imported = json!({"type":"object","additionalProperties":false,"required":["digest","path","kind"],"properties":{"digest":{"type":"string"},"path":{"type":"string"},"kind":{"enum":["directory","archive"]}}});
        let source = json!({"oneOf":[source,registry,imported]});
        let stream = json!({"type":"string","description":"Current discovery stream; required only for directory/archive imports"});
        let name = json!({"type":"string","description":"Stable standalone package name from discovery or manage_skills list"});
        let revision = json!({"type":"integer","minimum":1,"description":"Current installed revision from manage_skills list"});
        match self {
            Self::List => json!({"type":"object","additionalProperties":false,"properties":{}}),
            Self::Discover => {
                json!({"type":"object","additionalProperties":false,"properties":{
                "repository":{"type":"string"},"ref":{"type":["string","null"]},"path":{"type":["string","null"]},
                "registry":{"type":"object","additionalProperties":false,"required":["slug"],"properties":{"namespace":{"type":["string","null"]},"slug":{"type":"string"},"version":{"type":["string","null"]}}}},
                "oneOf":[
                    {"required":["repository"],"not":{"required":["registry"]}},
                    {"required":["registry"],"not":{"anyOf":[{"required":["repository"]},{"required":["ref"]},{"required":["path"]}]}},
                        {"required":["path"],"not":{"anyOf":[{"required":["repository"]},{"required":["registry"]},{"required":["ref"]}]}}
                ]})
            }
            Self::Install => {
                json!({"type":"object","additionalProperties":false,"required":["source","name"],"properties":{"source":source,"stream":stream,"path":{"type":"string"},"name":name}})
            }
            Self::Update => {
                json!({"type":"object","additionalProperties":false,"required":["source","name","expected_revision"],"properties":{"source":source,"stream":stream,"path":{"type":"string"},"name":name,"expected_revision":revision}})
            }
            Self::Uninstall => {
                json!({"type":"object","additionalProperties":false,"required":["name","expected_revision"],"properties":{"name":name,"expected_revision":revision}})
            }
        }
    }
    fn read_only(&self) -> bool {
        matches!(self, Self::List | Self::Discover)
    }
    fn command(&self, binding: &Binding, arguments: Value) -> Result<Command, Fault> {
        Ok(match self {
            Self::List => {
                let _: Empty = decode(arguments)?;
                Command::ListPlugins
            }
            Self::Discover => decode::<Discover>(arguments)?.command(binding.worktree)?,
            Self::Install => {
                let args: Install = decode(arguments)?;
                args.source.command(args.path, args.name, 0, args.stream)?
            }
            Self::Update => {
                let args: Update = decode(arguments)?;
                if args.expected_revision == 0 {
                    return Err(Fault::invalid(
                        "update requires an installed skill revision",
                    ));
                }
                args.source
                    .command(args.path, args.name, args.expected_revision, args.stream)?
            }
            Self::Uninstall => {
                let args: Remove = decode(arguments)?;
                if args.expected_revision == 0 {
                    return Err(sailry_protocol::Fault::invalid(
                        "uninstall requires an installed skill revision",
                    ));
                }
                Command::RemovePlugin {
                    name: args.name,
                    expected_revision: args.expected_revision,
                }
            }
        })
    }
    async fn prepare(&self, binding: &Binding, arguments: Value) -> Result<Command, Fault> {
        let command = self.command(binding, arguments)?;
        match &command {
            Command::InstallSkill {
                source,
                path,
                name,
                expected_revision,
            } if matches!(self, Self::Update) => {
                let info = standalone(binding, name, *expected_revision).await?;
                let previous = info
                    .skill
                    .as_ref()
                    .and_then(Provenance::git)
                    .ok_or_else(|| Fault::invalid("skill update must preserve its source kind"))?;
                if previous.source.repository != source.repository
                    || previous.source.git_ref != source.git_ref
                    || previous.path != *path
                {
                    return Err(sailry_protocol::Fault::invalid(
                        "skill update must preserve its source identity",
                    ));
                }
            }
            Command::InstallRegistrySkill {
                source,
                name,
                expected_revision,
            } if matches!(self, Self::Update) => {
                let info = standalone(binding, name, *expected_revision).await?;
                let Some(Provenance::Registry { registry }) = info.skill else {
                    return Err(Fault::invalid("skill update must preserve its source kind"));
                };
                if registry.namespace != source.namespace
                    || registry.slug != source.slug
                    || !semver::Version::parse(&source.version)
                        .map_err(|_| Fault::invalid("invalid skill version"))?
                        .cmp_precedence(
                            &semver::Version::parse(&registry.version)
                                .map_err(|_| Fault::invalid("invalid installed skill version"))?,
                        )
                        .is_gt()
                {
                    return Err(Fault::invalid(
                        "skill update requires the same coordinate and a newer version",
                    ));
                }
            }
            Command::InstallSkillUpload {
                source,
                name,
                expected_revision,
                ..
            } if matches!(self, Self::Update) => {
                let info = standalone(binding, name, *expected_revision).await?;
                let Some(Provenance::Imported { imported }) = info.skill else {
                    return Err(Fault::invalid("skill update must preserve its source kind"));
                };
                if imported.path != source.path {
                    return Err(Fault::invalid(
                        "skill reimport must preserve its directory identity",
                    ));
                }
            }
            Command::RemovePlugin {
                name,
                expected_revision,
            } => {
                standalone(binding, name, *expected_revision).await?;
            }
            _ => {}
        }
        Ok(command)
    }
    async fn output(&self, binding: &Binding, output: Output) -> Result<Value, Fault> {
        let value = match output {
            Output::Plugins(entries) if matches!(self, Self::List) => {
                let mut skills = Vec::new();
                for entry in entries {
                    let Output::Plugin(info) =
                        execute(binding, Command::ReadPlugin { name: entry.name }).await?
                    else {
                        return Err(sailry_protocol::Fault::invalid("expected a skill package"));
                    };
                    if let Some(source) = info.skill {
                        let provenance = match source {
                            Provenance::Git(source) => {
                                json!({"source":source.source,"path":source.path})
                            }
                            Provenance::Registry { registry } => json!({"source":registry}),
                            Provenance::Imported { imported } => json!({"source":imported}),
                        };
                        let mut value = json!({"name":info.summary.name,"revision":info.summary.revision,"enabled":info.summary.enabled,"skills":info.skills});
                        value
                            .as_object_mut()
                            .unwrap()
                            .extend(provenance.as_object().unwrap().clone());
                        skills.push(value);
                    }
                }
                json!({"skills":skills})
            }
            Output::SkillDiscovery(discovery) => serde_json::to_value(discovery).map_err(|_| {
                sailry_protocol::Fault::invalid("skill discovery could not be encoded")
            })?,
            Output::SkillImportDiscovery(discovery) => serde_json::to_value(discovery)
                .map_err(|_| Fault::invalid("skill discovery could not be encoded"))?,
            Output::RegistrySkillDiscovery(discovery) => serde_json::to_value(discovery)
                .map_err(|_| Fault::invalid("skill discovery could not be encoded"))?,
            Output::Plugin(info) => {
                json!({"name":info.summary.name,"revision":info.summary.revision,"enabled":info.summary.enabled,"source":info.skill,"skills":info.skills})
            }
            Output::Plugins(_) if matches!(self, Self::Uninstall) => json!({"removed":true}),
            _ => {
                return Err(sailry_protocol::Fault::invalid(
                    "unexpected skill management result",
                ));
            }
        };
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_registry_links_without_git_fields() {
        let args: Discover = decode(
            json!({"repository":"https://skillhub.cn/skills/example/analysis?version=1.2.0"}),
        )
        .unwrap();
        let Command::DiscoverRegistrySkill { source } =
            args.command(sailry_protocol::WorktreeId::new()).unwrap()
        else {
            panic!("registry discovery expected");
        };
        assert_eq!(source.namespace.as_deref(), Some("example"));
        assert_eq!(source.slug, "analysis");
        assert_eq!(source.version.as_deref(), Some("1.2.0"));
        let args: Discover = decode(json!({"registry":{"slug":"analysis"}})).unwrap();
        assert!(matches!(
            args.command(sailry_protocol::WorktreeId::new()).unwrap(),
            Command::DiscoverRegistrySkill { .. }
        ));
    }

    #[test]
    fn preserves_resolved_registry_identity() {
        let resolved = RegistryResolved {
            namespace: "example".into(),
            slug: "analysis".into(),
            version: "1.2.0".into(),
            version_id: "9".into(),
            content_hash: "a".repeat(64),
            package_md5: "b".repeat(32),
            archive_sha256: "c".repeat(64),
        };
        let args: Install = decode(json!({"source":resolved,"name":"skillhub-fixture"})).unwrap();
        let Command::InstallRegistrySkill {
            source,
            expected_revision,
            ..
        } = args
            .source
            .command(args.path, args.name, 0, args.stream)
            .unwrap()
        else {
            panic!("registry install expected");
        };
        assert_eq!(source, resolved);
        assert_eq!(expected_revision, 0);
        assert!(
            InstallSource::Registry(resolved)
                .command("skills/pretend-git".into(), "fixture".into(), 1, None)
                .is_err()
        );
    }
    #[test]
    fn confines_plan_actions() {
        let read_only = Management { read_only: true };
        assert!(read_only.arguments(json!({"action":"list"})).is_ok());
        assert!(
            read_only
                .arguments(json!({"action":"discover","path":"skills"}))
                .is_ok()
        );
        for action in ["install", "update", "uninstall"] {
            assert_eq!(
                read_only
                    .arguments(json!({"action":action}))
                    .err()
                    .unwrap()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        assert!(!Management { read_only: false }.read_only());
    }

    #[test]
    fn uses_fresh_import_streams() {
        let source = Imported {
            digest: "a".repeat(64),
            path: "analysis".into(),
            kind: plugin::UploadSource::Directory,
        };
        assert!(
            InstallSource::Imported(source.clone())
                .command(String::new(), "fixture".into(), 1, None)
                .is_err()
        );
        let stream = sailry_protocol::StreamId::new();
        let command = InstallSource::Imported(source.clone())
            .command(String::new(), "fixture".into(), 1, Some(stream))
            .unwrap();
        assert!(
            matches!(command,Command::InstallSkillUpload { source: captured,stream:actual,.. } if captured == source && actual == stream)
        );
    }
}
