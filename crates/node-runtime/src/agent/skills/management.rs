//! Standalone Skill management reuses the Node's inventory and durable package commands.
use super::super::tools::{Binding, Definition, decode};
use super::*;
use sailry_protocol::{
    Command, Output,
    plugin::{
        self,
        skills::{Resolved, Source},
    },
};

pub(in crate::agent) const NAMES: [&str; 5] = [
    "list_skills",
    "discover_skills",
    "install_skill",
    "update_skill",
    "uninstall_skill",
];

#[derive(Clone, Copy)]
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
        [
            Operation::List,
            Operation::Discover,
            Operation::Install,
            Operation::Update,
            Operation::Uninstall,
        ],
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Discover {
    repository: String,
    #[serde(rename = "ref")]
    git_ref: Option<String>,
    path: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Install {
    source: Resolved,
    path: String,
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Update {
    source: Resolved,
    path: String,
    name: String,
    expected_revision: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Remove {
    name: String,
    expected_revision: u64,
}

fn invalid(message: &str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
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
        return Err(invalid("expected a skill package"));
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
        NAMES[*self as usize]
    }
    fn description(&self) -> &'static str {
        match self {
            Self::List => {
                "List standalone skills installed on this execution Node, including their revision, enabled state and pinned Git source. This does not load skill instructions."
            }
            Self::Discover => {
                "Discover SKILL.md directories in a Git repository. Return the resolved commit and exact candidate path and name for install_skill or update_skill. This does not install or execute repository content. Use the execution Node's existing Git authentication; never provide credentials."
            }
            Self::Install => {
                "Install a new standalone skill using the exact source commit, path and name returned by discover_skills. This changes the Node inventory for new sessions or explicit skill selection; it does not change this turn's captured resources. Do not replace the commit with a branch or automatically retry an uncertain result."
            }
            Self::Update => {
                "Update an installed standalone skill using its current expected_revision from list_skills and a newly discovered exact commit. Keep its repository, Git ref, path and stable name unchanged. This changes the Node inventory, not existing session selections or this turn's captured skill version. Do not automatically replay an uncertain result."
            }
            Self::Uninstall => {
                "Uninstall a standalone skill by its stable name and current expected_revision from list_skills. Existing turns retain their captured version. This cannot uninstall ordinary feature plugins. Do not automatically replay an uncertain result."
            }
        }
    }
    fn parameters(&self) -> Value {
        let source = json!({"type":"object","additionalProperties":false,"required":["repository","git_ref","commit"],"properties":{
            "repository":{"type":"string","description":"Canonical repository from discovery, without credentials"},
            "git_ref":{"type":"string","description":"Git ref from discovery"},
            "commit":{"type":"string","pattern":"^[0-9a-f]{40}$","description":"Exact resolved commit from discovery"}
        }});
        let name = json!({"type":"string","description":"Stable standalone package name from discovery or list_skills"});
        let revision = json!({"type":"integer","minimum":1,"description":"Current installed revision from list_skills"});
        match self {
            Self::List => json!({"type":"object","additionalProperties":false,"properties":{}}),
            Self::Discover => {
                json!({"type":"object","additionalProperties":false,"required":["repository"],"properties":{
                    "repository":{"type":"string"},"ref":{"type":["string","null"]},"path":{"type":["string","null"]}
                }})
            }
            Self::Install => {
                json!({"type":"object","additionalProperties":false,"required":["source","path","name"],"properties":{"source":source,"path":{"type":"string"},"name":name}})
            }
            Self::Update => {
                json!({"type":"object","additionalProperties":false,"required":["source","path","name","expected_revision"],"properties":{"source":source,"path":{"type":"string"},"name":name,"expected_revision":revision}})
            }
            Self::Uninstall => {
                json!({"type":"object","additionalProperties":false,"required":["name","expected_revision"],"properties":{"name":name,"expected_revision":revision}})
            }
        }
    }
    fn read_only(&self) -> bool {
        matches!(self, Self::List | Self::Discover)
    }
    fn command(&self, _: &Binding, arguments: Value) -> Result<Command, Fault> {
        Ok(match self {
            Self::List => {
                let _: Empty = decode(arguments)?;
                Command::ListPlugins
            }
            Self::Discover => {
                let args: Discover = decode(arguments)?;
                Command::DiscoverSkills {
                    source: Source {
                        repository: args.repository,
                        git_ref: args.git_ref,
                        path: args.path,
                    },
                }
            }
            Self::Install => {
                let args: Install = decode(arguments)?;
                Command::InstallSkill {
                    source: args.source,
                    path: args.path,
                    name: args.name,
                    expected_revision: 0,
                }
            }
            Self::Update => {
                let args: Update = decode(arguments)?;
                if args.expected_revision == 0 {
                    return Err(invalid("update requires an installed skill revision"));
                }
                Command::InstallSkill {
                    source: args.source,
                    path: args.path,
                    name: args.name,
                    expected_revision: args.expected_revision,
                }
            }
            Self::Uninstall => {
                let args: Remove = decode(arguments)?;
                if args.expected_revision == 0 {
                    return Err(invalid("uninstall requires an installed skill revision"));
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
                let previous = info.skill.unwrap();
                if previous.source.repository != source.repository
                    || previous.source.git_ref != source.git_ref
                    || previous.path != *path
                {
                    return Err(invalid("skill update must preserve its source identity"));
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
                        return Err(invalid("expected a skill package"));
                    };
                    if let Some(source) = info.skill {
                        skills.push(json!({"name":info.summary.name,"revision":info.summary.revision,"enabled":info.summary.enabled,"source":source.source,"path":source.path,"skills":info.skills}));
                    }
                }
                json!({"skills":skills})
            }
            Output::SkillDiscovery(discovery) => serde_json::to_value(discovery)
                .map_err(|_| invalid("skill discovery could not be encoded"))?,
            Output::Plugin(info) => {
                json!({"name":info.summary.name,"revision":info.summary.revision,"enabled":info.summary.enabled,"source":info.skill,"skills":info.skills})
            }
            Output::Plugins(_) if matches!(self, Self::Uninstall) => json!({"removed":true}),
            _ => return Err(invalid("unexpected skill management result")),
        };
        Ok(value)
    }
}
