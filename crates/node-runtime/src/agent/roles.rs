//! Core catalog tools edit Node agents and teams. Application changes only the
//! conversation's future admission; running turns and children keep their captures.
use super::catalog::{
    Registration,
    display::{self, Label},
};
use super::tools::{self, Binding, Definition};
use super::*;
use async_trait::async_trait;
use sailry_protocol::{Command, Output, RoleId, TeamId, role, team, tool::Presentation};
use serde_json::{Map, Value, json};

pub(in crate::agent) const NAMES: [&str; 10] = [
    "list_agents",
    "add_agent",
    "edit_agent",
    "delete_agent",
    "apply_agents",
    "list_teams",
    "add_team",
    "edit_team",
    "delete_team",
    "apply_team",
];
/// Tools whose receipts carry the conversation revision for exact application.
pub(in crate::agent) const APPLY: [&str; 2] = ["apply_agents", "apply_team"];

const AGENT_FIELDS: [&str; 9] = [
    "key",
    "name",
    "description",
    "instructions",
    "read_only",
    "model",
    "max_turns",
    "skills",
    "appearance",
];
const TEAM_FIELDS: [&str; 7] = [
    "key",
    "name",
    "description",
    "instructions",
    "model",
    "members",
    "appearance",
];
const REVISION: &str = "^[1-9][0-9]*$";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    ListAgents,
    AddAgent,
    EditAgent,
    DeleteAgent,
    ApplyAgents,
    ListTeams,
    AddTeam,
    EditTeam,
    DeleteTeam,
    ApplyTeam,
}

const TOOLS: [Tool; 10] = [
    Tool::ListAgents,
    Tool::AddAgent,
    Tool::EditAgent,
    Tool::DeleteAgent,
    Tool::ApplyAgents,
    Tool::ListTeams,
    Tool::AddTeam,
    Tool::EditTeam,
    Tool::DeleteTeam,
    Tool::ApplyTeam,
];

/// Catalog editing belongs to project conversations, never to children or bound resources.
pub(in crate::agent) fn available(invocation: &Invocation) -> bool {
    invocation.child.is_none()
        && invocation.connections.bound.is_none()
        && invocation.project.is_some()
}

pub(in crate::agent) fn bind(
    ingress: &Arc<Ingress>,
    invocation: &Invocation,
    stop: &CancellationToken,
) -> Vec<Registration> {
    if !available(invocation) {
        return vec![];
    }
    tools::bind(Binding::new(ingress, invocation, stop), TOOLS)
}

#[async_trait]
impl Definition for Tool {
    fn name(&self) -> &'static str {
        NAMES[TOOLS
            .iter()
            .position(|tool| tool == self)
            .unwrap_or_default()]
    }

    fn description(&self) -> &'static str {
        match self {
            Self::ListAgents => {
                "List public agent definitions and exact revisions in this execution Node's catalog. Catalog edits affect new conversations only until explicitly applied; they never rewrite this turn's roster or existing children. No provider configuration or credentials are returned."
            }
            Self::AddAgent => {
                "Add an agent definition to this execution Node's catalog. Use a unique lowercase key, display name and instructions. Optional model identifies an existing provider/model, never credentials. This does not change this conversation's roster; use apply_agents only when explicitly requested."
            }
            Self::EditAgent => {
                "Edit one catalog agent using its exact ID and decimal-string revision from list_agents. Omitted fields are preserved; explicit null clears optional fields. Inspect conflicts instead of silently rebasing. Existing conversations, queued turns and children keep their captures until explicit application for future admission."
            }
            Self::DeleteAgent => {
                "Remove exactly one agent definition from this execution Node's catalog using its ID and decimal-string revision from list_agents. Confirm the user's intent before deletion. Existing conversations, queued turns and children retain their immutable definitions and history; this does not delete any conversation or automatically change its roster."
            }
            Self::ApplyAgents => {
                "Explicitly replace this conversation's selected agent roster for future turn admission only. Supply the complete desired list of catalog IDs with exact decimal-string revisions, and this conversation's exact expected_session_revision from initialization or a completed apply_agents receipt. An empty list clears future role selection. Never silently rebase conflicts. This running turn, already queued turns and existing children keep their immutable captures; this cannot target another conversation."
            }
            Self::ListTeams => {
                "List teams and exact revisions in this execution Node's catalog. A team selects catalog agents by ID and may add lead instructions and a lead model. The virtual Default team (key default) is not listed and always selects the whole agent catalog. No provider configuration or credentials are returned."
            }
            Self::AddTeam => {
                "Add a team to this execution Node's catalog. Use a unique lowercase key other than default, a display name and member agent IDs from list_agents. Optional instructions extend the lead's instructions; optional model identifies an existing provider/model, never credentials. This does not change this conversation's team; use apply_team only when explicitly requested."
            }
            Self::EditTeam => {
                "Edit one catalog team using its exact ID and decimal-string revision from list_teams. Omitted fields are preserved; explicit null clears optional fields; members replaces the complete member list. Inspect conflicts instead of silently rebasing. Existing conversations, queued turns and children keep their captures until explicit application for future admission."
            }
            Self::DeleteTeam => {
                "Remove exactly one team from this execution Node's catalog using its ID and decimal-string revision from list_teams. Confirm the user's intent before deletion. Member agents stay in the catalog. Existing conversations, queued turns and children retain their immutable captures; this does not delete any conversation or automatically change its team."
            }
            Self::ApplyTeam => {
                "Explicitly select this conversation's team for future turn admission only. Supply a team ID with its exact decimal-string revision from list_teams, or null for the Default team, which selects the whole agent catalog. Also supply this conversation's exact expected_session_revision from initialization or a completed apply receipt. Never silently rebase conflicts. This running turn, already queued turns and existing children keep their immutable captures; this cannot target another conversation."
            }
        }
    }

    fn parameters(&self) -> Value {
        let revision = json!({"type":"string","pattern":REVISION});
        let identity = json!({"id":{"type":"string"},"expected_revision":revision});
        let object = |required: Value, properties: Value| json!({"type":"object","additionalProperties":false,"required":required,"properties":properties});
        let merge = |mut base: Value, extra: &Value| {
            if let (Some(base), Some(extra)) = (base.as_object_mut(), extra.as_object()) {
                base.extend(extra.clone());
            }
            base
        };
        let agent = json!({
            "key":{"type":"string"},
            "name":{"type":"string"},
            "instructions":{"type":"string"},
            "description":{"type":"string"},
            "read_only":{"type":"boolean"},
            "model":{"type":["object","null"]},
            "max_turns":{"type":["integer","null"],"minimum":1},
            "skills":{"type":"array","items":{"type":"string"}},
            "appearance":{"type":["object","null"]}
        });
        let team = json!({
            "key":{"type":"string"},
            "name":{"type":"string"},
            "description":{"type":"string"},
            "instructions":{"type":"string"},
            "model":{"type":["object","null"]},
            "members":{"type":"array","items":{"type":"string"}},
            "appearance":{"type":["object","null"]}
        });
        let session = json!({"expected_session_revision":revision});
        match self {
            Self::ListAgents | Self::ListTeams => {
                json!({"type":"object","additionalProperties":false,"properties":{}})
            }
            Self::AddAgent => object(json!(["key", "name", "instructions"]), agent),
            Self::EditAgent => object(json!(["id", "expected_revision"]), merge(identity, &agent)),
            Self::DeleteAgent | Self::DeleteTeam => {
                object(json!(["id", "expected_revision"]), identity)
            }
            Self::ApplyAgents => object(
                json!(["agents", "expected_session_revision"]),
                merge(
                    json!({"agents":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["id","revision"],"properties":{"id":{"type":"string"},"revision":revision}}}}),
                    &session,
                ),
            ),
            Self::AddTeam => object(json!(["key", "name"]), team),
            Self::EditTeam => object(json!(["id", "expected_revision"]), merge(identity, &team)),
            Self::ApplyTeam => object(
                json!(["team", "expected_session_revision"]),
                merge(
                    json!({"team":{"type":["object","null"],"additionalProperties":false,"required":["id","revision"],"properties":{"id":{"type":"string"},"revision":revision}}}),
                    &session,
                ),
            ),
        }
    }

    fn read_only(&self) -> bool {
        matches!(self, Self::ListAgents | Self::ListTeams)
    }

    fn presentation(&self) -> Presentation {
        if self.read_only() {
            Presentation::Summary
        } else {
            Presentation::Details
        }
    }

    fn display(&self) -> Option<sailry_protocol::tool::Display> {
        let summary = |path: &str| json!({"summary":{"path":path}});
        Some(match self {
            Self::ListAgents => display::team(Label::ListAgents, Value::Null),
            Self::AddAgent => display::team(Label::AddAgent, summary("/name")),
            Self::EditAgent => display::team(Label::EditAgent, summary("/id")),
            Self::DeleteAgent => display::team(Label::DeleteAgent, summary("/id")),
            Self::ApplyAgents => display::team(Label::ApplyAgents, Value::Null),
            Self::ListTeams => display::team(Label::ListTeams, Value::Null),
            Self::AddTeam => display::team(Label::AddTeam, summary("/name")),
            Self::EditTeam => display::team(Label::EditTeam, summary("/id")),
            Self::DeleteTeam => display::team(Label::DeleteTeam, summary("/id")),
            Self::ApplyTeam => display::team(Label::ApplyTeam, Value::Null),
        })
    }

    fn command(&self, binding: &Binding, arguments: Value) -> Result<Command, Fault> {
        command(*self, &binding.session, arguments, None)
    }

    async fn prepare(&self, binding: &Binding, arguments: Value) -> Result<Command, Fault> {
        let current = match self {
            Self::EditAgent => Some(execute(binding, Command::ListRoles).await?),
            Self::EditTeam => Some(execute(binding, Command::ListTeams).await?),
            _ => None,
        };
        command(*self, &binding.session, arguments, current)
    }

    async fn output(&self, _: &Binding, output: Output) -> Result<Value, Fault> {
        project(*self, output)
    }
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
        .map_err(|_| Fault::new(ErrorCode::Unavailable, "catalog response is unavailable"))?
}

/// Decimal-string revisions keep exact 64-bit values for script-safe models.
fn revision(value: Option<&Value>) -> Option<u64> {
    let text = value?.as_str()?;
    let number = text.parse::<u64>().ok()?;
    (number > 0 && number.to_string() == text && number <= i64::MAX as u64).then_some(number)
}

fn fields<'a>(
    arguments: &'a Value,
    allowed: &[&str],
    required: &[&str],
) -> Option<&'a Map<String, Value>> {
    let input = arguments.as_object()?;
    (input.keys().all(|key| allowed.contains(&key.as_str()))
        && required.iter().all(|key| input.contains_key(*key)))
    .then_some(input)
}

fn changes(input: &Map<String, Value>, allowed: &[&str]) -> Map<String, Value> {
    input
        .iter()
        .filter(|(key, _)| allowed.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn merged<T: serde::de::DeserializeOwned>(
    mut base: Value,
    input: Map<String, Value>,
) -> Result<T, Fault> {
    if let Some(base) = base.as_object_mut() {
        base.extend(input);
    }
    serde_json::from_value(base).map_err(|_| Fault::invalid("invalid catalog arguments"))
}

fn identity<T: serde::de::DeserializeOwned>(input: &Map<String, Value>) -> Result<T, Fault> {
    input
        .get("id")
        .cloned()
        .and_then(|id| serde_json::from_value(id).ok())
        .ok_or_else(|| Fault::invalid("invalid catalog ID"))
}

fn command(
    tool: Tool,
    session: &str,
    arguments: Value,
    current: Option<Output>,
) -> Result<Command, Fault> {
    let conversation = || {
        session
            .parse()
            .map_err(|_| Fault::invalid("application requires this conversation"))
    };
    let edit = |allowed: &[&str], message: &str| {
        let mut keys = allowed.to_vec();
        keys.extend(["id", "expected_revision"]);
        fields(&arguments, &keys, &["id", "expected_revision"])
            .filter(|input| allowed.iter().any(|key| input.contains_key(*key)))
            .and_then(|input| Some((input, revision(input.get("expected_revision"))?)))
            .ok_or_else(|| Fault::invalid(message))
    };
    let removal = |message: &str| {
        fields(
            &arguments,
            &["id", "expected_revision"],
            &["id", "expected_revision"],
        )
        .and_then(|input| Some((input, revision(input.get("expected_revision"))?)))
        .ok_or_else(|| Fault::invalid(message))
    };
    let changed = || Fault::new(ErrorCode::RevisionConflict, "catalog entry changed");
    Ok(match tool {
        Tool::ListAgents | Tool::ListTeams => {
            fields(&arguments, &[], &[])
                .ok_or_else(|| Fault::invalid("Invalid catalog arguments"))?;
            if tool == Tool::ListAgents {
                Command::ListRoles
            } else {
                Command::ListTeams
            }
        }
        Tool::AddAgent => {
            let input = fields(&arguments, &AGENT_FIELDS, &["key", "name", "instructions"])
                .ok_or_else(|| Fault::invalid("Invalid agent definition"))?;
            let base = json!({"id":RoleId::new(),"revision":0,"key":"","name":"","description":"","instructions":"",
                "read_only":false,"model":null,"max_turns":null,"skills":[],"appearance":null});
            let role: role::Profile = merged(base, changes(input, &AGENT_FIELDS))?;
            role.validate()?;
            Command::PutRole {
                role,
                expected_revision: 0,
            }
        }
        Tool::EditAgent => {
            let (input, expected) = edit(
                &AGENT_FIELDS,
                "Inspect the agent and use its exact revision",
            )?;
            let id: RoleId = identity(input)?;
            let Some(Output::Roles(roles)) = current else {
                return Err(Fault::new(
                    ErrorCode::Internal,
                    "agent catalog is unavailable",
                ));
            };
            let current = roles
                .into_iter()
                .find(|role| role.id == id)
                .ok_or_else(|| Fault::new(ErrorCode::NotFound, "Agent not found"))?;
            if current.revision != expected {
                return Err(changed());
            }
            let role: role::Profile = merged(json!(current), changes(input, &AGENT_FIELDS))?;
            role.validate()?;
            Command::PutRole {
                role,
                expected_revision: expected,
            }
        }
        Tool::DeleteAgent => {
            let (input, expected) = removal("Inspect the agent and use its exact revision")?;
            Command::RemoveRole {
                role: identity(input)?,
                expected_revision: expected,
            }
        }
        Tool::ApplyAgents => {
            let invalid = || Fault::invalid("Use distinct agents and exact revisions");
            let input = fields(
                &arguments,
                &["agents", "expected_session_revision"],
                &["agents", "expected_session_revision"],
            )
            .ok_or_else(invalid)?;
            let expected = revision(input.get("expected_session_revision")).ok_or_else(invalid)?;
            let roles = input
                .get("agents")
                .and_then(Value::as_array)
                .ok_or_else(invalid)?
                .iter()
                .map(|entry| {
                    let entry = fields(entry, &["id", "revision"], &["id", "revision"])
                        .ok_or_else(invalid)?;
                    Ok(role::Reference {
                        id: identity(entry).map_err(|_| invalid())?,
                        revision: revision(entry.get("revision")).ok_or_else(invalid)?,
                    })
                })
                .collect::<Result<Vec<_>, Fault>>()?;
            let ids: std::collections::BTreeSet<_> = roles.iter().map(|role| role.id).collect();
            if ids.len() != roles.len() {
                return Err(invalid());
            }
            Command::SetSessionRoles {
                session: conversation()?,
                expected_revision: expected,
                roles,
            }
        }
        Tool::AddTeam => {
            let input = fields(&arguments, &TEAM_FIELDS, &["key", "name"])
                .ok_or_else(|| Fault::invalid("Invalid team definition"))?;
            let base = json!({"id":TeamId::new(),"revision":0,"key":"","name":"","appearance":null,
                "description":"","model":null,"instructions":"","members":[]});
            let team: team::Profile = merged(base, changes(input, &TEAM_FIELDS))?;
            team.validate()?;
            Command::PutTeam {
                team,
                expected_revision: 0,
            }
        }
        Tool::EditTeam => {
            let (input, expected) =
                edit(&TEAM_FIELDS, "Inspect the team and use its exact revision")?;
            let id: TeamId = identity(input)?;
            let Some(Output::Teams(teams)) = current else {
                return Err(Fault::new(
                    ErrorCode::Internal,
                    "team catalog is unavailable",
                ));
            };
            let current = teams
                .into_iter()
                .find(|team| team.id == id)
                .ok_or_else(|| Fault::new(ErrorCode::NotFound, "Team not found"))?;
            if current.revision != expected {
                return Err(changed());
            }
            let team: team::Profile = merged(json!(current), changes(input, &TEAM_FIELDS))?;
            team.validate()?;
            Command::PutTeam {
                team,
                expected_revision: expected,
            }
        }
        Tool::DeleteTeam => {
            let (input, expected) = removal("Inspect the team and use its exact revision")?;
            Command::RemoveTeam {
                team: identity(input)?,
                expected_revision: expected,
            }
        }
        Tool::ApplyTeam => {
            let invalid = || Fault::invalid("Use a listed team and exact revisions");
            let input = fields(
                &arguments,
                &["team", "expected_session_revision"],
                &["team", "expected_session_revision"],
            )
            .ok_or_else(invalid)?;
            let expected = revision(input.get("expected_session_revision")).ok_or_else(invalid)?;
            let team = match input.get("team") {
                Some(Value::Null) | None => None,
                Some(entry) => {
                    let entry = fields(entry, &["id", "revision"], &["id", "revision"])
                        .ok_or_else(invalid)?;
                    Some(team::Reference {
                        id: identity(entry).map_err(|_| invalid())?,
                        revision: revision(entry.get("revision")).ok_or_else(invalid)?,
                    })
                }
            };
            Command::SetSessionExecution {
                execution: sailry_protocol::conversation::Execution::Team,
                models: Default::default(),
                session: conversation()?,
                expected_revision: expected,
                team: team.map(Into::into),
            }
        }
    })
}

fn exact<T: serde::Serialize>(value: &T, revision: u64) -> Value {
    let mut value = json!(value);
    value["revision"] = json!(revision.to_string());
    value
}

/// Public definitions only; frozen provider configuration never leaves the store.
fn project(tool: Tool, output: Output) -> Result<Value, Fault> {
    let agents = |roles: Vec<role::Profile>| -> Vec<Value> {
        roles
            .iter()
            .map(|role| exact(role, role.revision))
            .collect()
    };
    let teams = |teams: Vec<team::Profile>| -> Vec<Value> {
        teams
            .iter()
            .map(|team| exact(team, team.revision))
            .collect()
    };
    Ok(match (tool, output) {
        (Tool::ListAgents, Output::Roles(roles)) => json!({"agents":agents(roles)}),
        (Tool::DeleteAgent, Output::Roles(roles)) => json!(agents(roles)),
        (Tool::AddAgent | Tool::EditAgent, Output::Role(role)) => exact(&role, role.revision),
        (Tool::ListTeams, Output::Teams(list)) => json!({"teams":teams(list)}),
        (Tool::DeleteTeam, Output::Teams(list)) => json!(teams(list)),
        (Tool::AddTeam | Tool::EditTeam, Output::Team(team)) => exact(&team, team.revision),
        (Tool::ApplyAgents | Tool::ApplyTeam, Output::Session(session)) => {
            let roles: Vec<_> = session
                .roles
                .profiles
                .iter()
                .map(|role| json!({"id":role.id,"revision":role.revision.to_string()}))
                .collect();
            let mut receipt =
                json!({"session":session.id,"revision":session.revision.to_string(),"roles":roles});
            if tool == Tool::ApplyTeam {
                receipt["team"] = session.roles.team.map_or(
                    Value::Null,
                    |team| json!({"id":team.id,"revision":team.revision.to_string()}),
                );
            }
            receipt
        }
        _ => {
            return Err(Fault::new(
                ErrorCode::Internal,
                "catalog outcome is unavailable",
            ));
        }
    })
}

#[cfg(test)]
mod tests;
