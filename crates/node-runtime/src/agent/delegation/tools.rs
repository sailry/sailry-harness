//! Core delegation tools start, guide and observe children owned by this conversation.
use super::*;
use crate::agent::catalog::{
    Registration,
    display::{self, Label},
};
use sailry_protocol::tool::{Grouping, Operation, Presentation};

pub(crate) const SPAWN: &str = "spawn_agent";
pub(crate) const GUIDE: &str = "guide_agent";
pub(crate) const WAIT: &str = "wait_agent";
pub(in crate::agent) const NAMES: [&str; 3] = [SPAWN, GUIDE, WAIT];
/// Turn-context label that coordinator instructions cite.
pub(in crate::agent) const BUDGET: &str = "Concurrency budget";
/// Budgets at or above this never queue children in practice.
const UNBOUNDED: u64 = u32::MAX as u64;

const SPAWN_DESCRIPTION: &str = "Delegate bounded parallel work when the user or the active work mode allows it (project guides do not). Keep immediate lookups and tightly coupled steps yourself. Issue all independent spawn_agent calls of one wave in the same response; foreground children from one response run concurrently and this conversation resumes after all finish, while foreground calls in separate responses run sequentially. Supply the goal, relevant paths and excerpts, constraints, edit ownership and expected result; the child cannot see this conversation. Children skip builds and test suites unless the task assigns verification; run consolidated checks after integrating their results. Expect a summary with path:line pointers, not file contents. The default uses a bounded observation window; verify completed results before relying on them. An admitted, pending child keeps running under its returned session and turn IDs. Do not spawn it again. Use background only for work this turn does not need; completion arrives automatically. Do not poll or claim uncollected results were integrated. Queued admission is not execution or completion. Use guide_agent for new directions. Children inherit this turn's worktree, including a private worktree, mode and permission and cannot delegate. Choose an exact listed role, or omit it to inherit the model and instructions. worktree selects a registered project checkout; concurrent edits need disjoint files or separate worktrees. To reuse an idle child owned by this conversation, use its exact returned session ID and normally omit role; never reconstruct it. It keeps its history, role, model, worktree and settings.";
const GUIDE_DESCRIPTION: &str = "Give new directions to a child owned by this conversation. Copy its exact returned session ID; never reconstruct it. Running guidance is durable: the child receives it after its entire current tool batch settles and before its next model request; an in-flight model response yields at that boundary. This returns a queued guidance receipt, not the child's final result. Explicit follow-up to an idle child starts a new task in the same session, preserving its history, role, model, worktree and permissions. The task uses current package settings and the normal bounded child observation window; a pending result keeps running under its returned session and turn IDs. Do not spawn it again. Busy queued or stopping children cannot be restarted.";
const WAIT_DESCRIPTION: &str = "Observe a child owned by this conversation within a bounded observation window and collect its durable final status and answer when available. An admitted, pending response with queued or running status is not failure or completion: the same child continues under its existing session and turn IDs. Do not spawn it again. Use it when the user requests waiting or result collection, or to collect the complete answer from a background notice. Do not poll running background work; its completion arrives as a notice. Guiding background work alone does not request waiting. Copy its exact returned session ID; never reconstruct it. Main-session guidance returns early with guidance_pending so this conversation can apply the user's direction; resume waiting only if the user still requests it. This does not create or replay work.";
const UUID: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$";

/// Children and resource-bound conversations cannot create or control other agents.
pub(in crate::agent) fn available(invocation: &Invocation) -> bool {
    invocation.child.is_none()
        && invocation.connections.bound.is_none()
        && invocation.turn.roles.execution == sailry_protocol::conversation::Execution::Team
}

pub(in crate::agent) fn bind(
    ingress: &Arc<Ingress>,
    tasks: &Tasks,
    invocation: &Invocation,
    stop: &CancellationToken,
) -> Vec<Registration> {
    if !available(invocation) {
        return vec![];
    }
    let delegate = |name: &str, description: &str, parameters: Value| {
        Delegate::new(
            ingress,
            tasks,
            invocation,
            stop,
            name.to_owned(),
            description.to_owned(),
            parameters,
        )
    };
    let spawn = Arc::new(Spawn {
        delegate: delegate(SPAWN, SPAWN_DESCRIPTION, parameters()),
        roles: invocation
            .turn
            .roles
            .profiles
            .iter()
            .map(|role| role.key.clone())
            .collect(),
    });
    let mut registration = Registration::managed(spawn.descriptor(), spawn);
    registration.delegation = true;
    registration.grouping = Grouping::Standalone;
    let mut tools = vec![registration];
    for (name, description, operation) in [
        (GUIDE, GUIDE_DESCRIPTION, Operation::GuideAgent),
        (WAIT, WAIT_DESCRIPTION, Operation::WaitAgent),
    ] {
        let control = Arc::new(Control::new(
            ingress,
            invocation,
            None,
            stop,
            operation,
            delegate(name, description, Value::Null),
        ));
        tools.push(Registration::managed(control.descriptor(), control));
    }
    for registration in &mut tools {
        let name = registration.tool.name.to_string();
        registration.presentation = Presentation::Details;
        registration.display = Some((name.clone(), card(&name)));
    }
    tools
}

fn card(name: &str) -> sailry_protocol::tool::Display {
    match name {
        SPAWN => display::team(
            Label::Subagent,
            json!({"summary":{"path":"/title"},"context":{"path":"/role"},"content":{"path":"/task"}}),
        ),
        GUIDE => display::team(
            Label::GuideAgent,
            json!({"summary":{"path":"/session","format":"session"},"content":{"path":"/message"}}),
        ),
        _ => display::team(
            Label::WaitAgent,
            json!({"summary":{"path":"/session","format":"session"}}),
        ),
    }
}

fn parameters() -> Value {
    json!({
        "type":"object","additionalProperties":false,"required":["task"],
        "properties":{
            "session":{"type":"string","pattern":UUID},
            "role":{"type":["string","null"],"description":"Exact role key from the captured conversation roster; omit or use null to inherit the parent model and instructions"},
            "task":{"type":"string"},
            "title":{"type":"string","minLength":1},
            "background":{"type":"boolean"},
            "worktree":{"type":["string","null"]}
        }
    })
}

/// Selection metadata for the captured roster; delivered as turn context.
/// Roster and revision lines follow registration because discovery can declare
/// those tools mid-turn; the budget follows `mounted`, this request's declarations.
pub(in crate::agent) fn context(
    invocation: &Invocation,
    tools: &[Registration],
    mounted: impl Fn(&str) -> bool,
) -> String {
    let registered = |name: &str| tools.iter().any(|entry| entry.tool.name.as_ref() == name);
    let roster: Vec<_> = invocation
        .turn
        .roles
        .profiles
        .iter()
        .map(|role| {
            let purpose = role.description.trim();
            format!(
                "{} ({}){}{}",
                role.key,
                role.name,
                if role.read_only { " [read only]" } else { "" },
                if purpose.is_empty() {
                    String::new()
                } else {
                    format!(": {purpose}")
                }
            )
        })
        .collect();
    let mut text = if !registered(SPAWN) {
        String::new()
    } else if roster.is_empty() {
        "No delegation roles are configured".to_owned()
    } else {
        format!(
            "Configured delegation roles (selection metadata only):\n{}",
            roster.join("\n")
        )
    };
    if mounted(SPAWN) {
        let limit = invocation.child_concurrency;
        text.push_str(&if limit >= UNBOUNDED {
            format!("\n{BUDGET}: no limit on children running at once in this conversation.")
        } else {
            format!(
                "\n{BUDGET}: up to {limit} children run at once in this conversation; later children queue in FIFO order."
            )
        });
    }
    if super::super::roles::APPLY
        .iter()
        .any(|name| registered(name))
    {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&format!(
            "Captured conversation revision: {}. Catalog changes do not change this turn's roster. Explicit application affects future admission only; use the exact inspected revision and never silently rebase a conflict.",
            invocation.turn.revision
        ));
    }
    text
}

struct Spawn {
    delegate: Delegate,
    roles: Vec<String>,
}

impl Spawn {
    fn name(&self) -> &str {
        self.delegate.name()
    }
    fn description(&self) -> &str {
        self.delegate.description()
    }
    fn parameters_schema(&self) -> Option<Value> {
        self.delegate.parameters_schema()
    }
    fn is_read_only(&self) -> bool {
        false
    }
    async fn execute(
        &self,
        context: Arc<CallContext>,
        arguments: Value,
    ) -> Result<rmcp::model::CallToolResult, Fault> {
        if let Err(error) = prepare(&arguments, &self.roles) {
            return crate::agent::catalog::json(json!({"isError":true,"error":error}));
        }
        self.delegate
            .execute_prepared(context, arguments.clone(), arguments)
            .await
    }
}

crate::agent::catalog::provider!(Spawn, crate::agent::catalog::result);

/// Rejects malformed requests before admission; the store still owns exact checks.
fn prepare(arguments: &Value, roles: &[String]) -> Result<(), Fault> {
    const FIELDS: [&str; 6] = ["session", "role", "task", "title", "worktree", "background"];
    let uuid = |value: &str| {
        let parts: Vec<_> = value.split('-').collect();
        parts.iter().map(|part| part.len()).eq([8, 4, 4, 4, 12])
            && parts.iter().all(|part| {
                part.bytes().all(|byte| {
                    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte.to_ascii_lowercase())
                })
            })
    };
    let Some(fields) = arguments.as_object() else {
        return Err(Fault::invalid("Invalid delegation arguments"));
    };
    if fields.keys().any(|key| !FIELDS.contains(&key.as_str())) {
        return Err(Fault::invalid("Invalid delegation arguments"));
    }
    if fields
        .get("background")
        .is_some_and(|value| !value.is_boolean())
    {
        return Err(Fault::invalid("Invalid delegation background mode"));
    }
    if fields
        .get("session")
        .is_some_and(|value| !value.as_str().is_some_and(uuid))
    {
        return Err(Fault::invalid("Invalid child session ID"));
    }
    if fields
        .get("role")
        .is_some_and(|value| !value.is_null() && !roles.iter().any(|role| value == role))
    {
        return Err(Fault::invalid("Select a role listed in this tool"));
    }
    if !fields
        .get("task")
        .and_then(Value::as_str)
        .is_some_and(|task| !task.trim().is_empty())
    {
        return Err(Fault::invalid("A delegation task is required"));
    }
    if fields.get("title").is_some_and(|value| {
        !value.is_null()
            && !value
                .as_str()
                .is_some_and(|title| !title.trim().is_empty() && !title.contains(['\n', '\r']))
    }) {
        return Err(Fault::invalid("Invalid delegation title"));
    }
    if fields
        .get("worktree")
        .is_some_and(|value| !value.is_null() && !value.as_str().is_some_and(uuid))
    {
        return Err(Fault::invalid("Invalid worktree ID"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
