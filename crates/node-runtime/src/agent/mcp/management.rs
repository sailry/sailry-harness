//! Standalone MCP management uses the existing inventory and durable installer.
use super::super::tools::{self, Binding, Definition, decode};
use super::*;
use sailry_protocol::{
    Command, Output,
    plugin::{self, mcp::Definition as Server, settings::SecretUpdate},
};
use serde::Deserialize;

pub(in crate::agent) const NAMES: [&str; 1] = ["manage_mcp"];

#[derive(Clone, Copy)]
enum Operation {
    List,
    Install,
    Update,
    Uninstall,
}

pub(in crate::agent) fn bind(
    ingress: &Arc<Ingress>,
    invocation: &Invocation,
    stop: &CancellationToken,
) -> Vec<Registration> {
    tools::bind(
        Binding::new(ingress, invocation, stop),
        [Management {
            plan: invocation.turn.config.mode == sailry_protocol::WorkMode::Plan,
        }],
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Install {
    name: String,
    definition: Server,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Update {
    name: String,
    expected_revision: u64,
    definition: Option<Server>,
    #[serde(default)]
    clear_credentials: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Remove {
    name: String,
    expected_revision: u64,
}

fn install(args: Install) -> Command {
    let secrets = args
        .definition
        .slots()
        .iter()
        .map(|name| (name.clone(), SecretUpdate::Clear))
        .collect();
    Command::InstallMcp {
        name: args.name,
        expected_revision: 0,
        definition: args.definition,
        secrets,
    }
}

fn standalone(info: &plugin::Info, revision: u64) -> Result<&Server, Fault> {
    let definition = info.mcp_source.as_ref().ok_or_else(|| {
        Fault::new(
            ErrorCode::PermissionDenied,
            "only standalone MCP packages can be managed by this tool",
        )
    })?;
    if revision == 0 || info.summary.revision != revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "MCP server changed; reload its revision",
        ));
    }
    Ok(definition)
}

fn update(args: Update, previous: &Server) -> Result<Command, Fault> {
    let definition = args.definition.unwrap_or_else(|| previous.clone());
    // Retain the existing credential bindings. Adding or changing slots belongs
    // in the protected configuration editor, never model-visible arguments.
    let slots: BTreeSet<_> = definition.slots().iter().collect();
    if definition.transport() != previous.transport()
        || slots != previous.slots().iter().collect()
        || slots.len() != definition.slots().len()
    {
        return Err(Fault::invalid(
            "MCP updates must preserve transport and credential slots",
        ));
    }
    let clear: BTreeSet<_> = args.clear_credentials.iter().collect();
    if clear.len() != args.clear_credentials.len() || !clear.is_subset(&slots) {
        return Err(Fault::invalid("unknown or repeated MCP credential slot"));
    }
    let secrets = definition
        .slots()
        .iter()
        .map(|name| {
            (
                name.clone(),
                if clear.contains(name) {
                    SecretUpdate::Clear
                } else {
                    SecretUpdate::Keep
                },
            )
        })
        .collect();
    Ok(Command::InstallMcp {
        name: args.name,
        expected_revision: args.expected_revision,
        definition,
        secrets,
    })
}

fn summary(info: &plugin::Info) -> Value {
    json!({"name":info.summary.name,"revision":info.summary.revision,
        "enabled":info.summary.enabled,"standalone":info.mcp_source.is_some(),
        "servers":info.mcp.iter().map(|server| json!({
            "name":server.name,"transport":server.transport
        })).collect::<Vec<_>>(),
        "credential_slots":info.mcp_source.as_ref().map(Server::slots).unwrap_or_default()})
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
                "MCP inventory response is unavailable",
            )
        })?
}

async fn read(binding: &Binding, name: &str) -> Result<plugin::Info, Fault> {
    match execute(binding, Command::ReadPlugin { name: name.into() }).await? {
        Output::Plugin(info) => Ok(info),
        _ => Err(Fault::invalid("expected an MCP package")),
    }
}

impl Operation {
    fn parameters(&self) -> Value {
        let slots = json!({"type":"array","items":{"type":"string"},"uniqueItems":true,"description":"Credential names only, never values"});
        let definition = json!({"oneOf":[
            {"type":"object","additionalProperties":false,"required":["type","command","args","env"],"properties":{"type":{"const":"stdio"},"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}},"env":slots}},
            {"type":"object","additionalProperties":false,"required":["type","url","headers"],"properties":{"type":{"enum":["streamable-http","sse"]},"url":{"type":"string"},"headers":slots}}
        ]});
        let name = json!({"type":"string","pattern":"^mcp-.+"});
        let revision = json!({"type":"integer","minimum":1});
        match self {
            Self::List => json!({"type":"object","additionalProperties":false,"properties":{}}),
            Self::Install => {
                json!({"type":"object","additionalProperties":false,"required":["name","definition"],"properties":{"name":name,"definition":definition}})
            }
            Self::Update => {
                json!({"type":"object","additionalProperties":false,"required":["name","expected_revision"],"properties":{"name":name,"expected_revision":revision,"definition":definition,"clear_credentials":slots}})
            }
            Self::Uninstall => {
                json!({"type":"object","additionalProperties":false,"required":["name","expected_revision"],"properties":{"name":name,"expected_revision":revision}})
            }
        }
    }

    fn read_only(&self) -> bool {
        matches!(self, Self::List)
    }

    fn command(&self, _: &Binding, arguments: Value) -> Result<Command, Fault> {
        match self {
            Self::List => {
                let _: Empty = decode(arguments)?;
                Ok(Command::ListPlugins)
            }
            Self::Install => Ok(install(decode(arguments)?)),
            // Preparation reads the current immutable definition before mutation.
            Self::Update | Self::Uninstall => {
                Err(Fault::invalid("MCP mutation requires an installed package"))
            }
        }
    }

    async fn prepare(&self, binding: &Binding, arguments: Value) -> Result<Command, Fault> {
        match self {
            Self::Update => {
                let args: Update = decode(arguments)?;
                let info = read(binding, &args.name).await?;
                let previous = standalone(&info, args.expected_revision)?;
                update(args, previous)
            }
            Self::Uninstall => {
                let args: Remove = decode(arguments)?;
                let info = read(binding, &args.name).await?;
                standalone(&info, args.expected_revision)?;
                Ok(Command::RemovePlugin {
                    name: args.name,
                    expected_revision: args.expected_revision,
                })
            }
            _ => self.command(binding, arguments),
        }
    }
}

struct Management {
    plan: bool,
}

impl Management {
    fn arguments(&self, mut arguments: Value) -> Result<(Operation, Value), Fault> {
        let action = arguments
            .as_object_mut()
            .and_then(|args| args.remove("action"))
            .ok_or_else(|| Fault::invalid("MCP action is required"))?;
        let operation = match action.as_str() {
            Some("list") => Operation::List,
            Some("install") => Operation::Install,
            Some("update") => Operation::Update,
            Some("uninstall") => Operation::Uninstall,
            _ => return Err(Fault::invalid("unknown MCP action")),
        };
        if self.plan && !operation.read_only() {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "MCP mutations are unavailable in Plan mode",
            ));
        }
        Ok((operation, arguments))
    }
}

#[async_trait]
impl Definition for Management {
    fn name(&self) -> &'static str {
        NAMES[0]
    }
    fn description(&self) -> &'static str {
        "Manage MCP servers on this execution Node: list declarations without starting servers, or install/update/uninstall standalone entries using their current revision. Definition env/headers contain credential slot names only. Never include private values in tool arguments, URLs or command arguments; use the protected configuration form. Existing secrets and enabled state are retained on update unless clear_credentials explicitly names a slot. OAuth still requires interactive authorization. Plan permits only list. Changes affect future captures, not this turn's tools. Do not replay uncertain mutations."
    }
    fn parameters(&self) -> Value {
        let operations = if self.plan {
            vec![("list", Operation::List)]
        } else {
            vec![
                ("list", Operation::List),
                ("install", Operation::Install),
                ("update", Operation::Update),
                ("uninstall", Operation::Uninstall),
            ]
        };
        let schemas: Vec<_> = operations
            .into_iter()
            .map(|(action, operation)| {
                let mut schema = operation.parameters();
                schema["properties"]["action"] = json!({"const":action});
                let mut required = schema
                    .get("required")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                required.push(json!("action"));
                schema["required"] = json!(required);
                schema
            })
            .collect();
        let mut properties = serde_json::Map::new();
        for schema in &schemas {
            if let Some(fields) = schema["properties"].as_object() {
                properties.extend(fields.clone());
            }
        }
        properties.insert("action".into(), json!({"type":"string","enum":schemas.iter().map(|schema| schema["properties"]["action"]["const"].clone()).collect::<Vec<_>>()}));
        json!({"type":"object","properties":properties,"required":["action"],"additionalProperties":false,"oneOf":schemas})
    }
    fn read_only(&self) -> bool {
        self.plan
    }
    fn command(&self, binding: &Binding, arguments: Value) -> Result<Command, Fault> {
        let (operation, arguments) = self.arguments(arguments)?;
        operation.command(binding, arguments)
    }
    async fn prepare(&self, binding: &Binding, arguments: Value) -> Result<Command, Fault> {
        let (operation, arguments) = self.arguments(arguments)?;
        operation.prepare(binding, arguments).await
    }
    async fn output(&self, binding: &Binding, output: Output) -> Result<Value, Fault> {
        match output {
            Output::Plugins(entries) => {
                let mut servers = Vec::new();
                for entry in entries {
                    let info = read(binding, &entry.name).await?;
                    if !info.mcp.is_empty() {
                        servers.push(summary(&info));
                    }
                }
                Ok(json!({"servers":servers}))
            }
            Output::Plugin(info) => Ok(summary(&info)),
            _ => Err(Fault::invalid("unexpected MCP management result")),
        }
    }
}

#[cfg(test)]
mod tests;
