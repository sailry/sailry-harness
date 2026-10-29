//! Skill loading uses the captured Node package resources and canonical context.
use super::*;
use crate::agent::catalog::CallContext;
use crate::plugins::resources::Resources;
use sailry_protocol::tool::{Display, Presentation};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::OnceLock;
pub(super) mod management;

pub(super) const NAMES: [&str; 2] = ["load_skill", "read_skill_resource"];

struct Skills {
    resources: Arc<Resources>,
    session: String,
    stop: CancellationToken,
    resource: bool,
    ingress: Arc<Ingress>,
    turn: sailry_protocol::TurnId,
}

impl Skills {
    fn display(resource: bool) -> &'static Display {
        static DISPLAYS: OnceLock<[Display; 2]> = OnceLock::new();
        &DISPLAYS.get_or_init(|| {
            [
                catalog::display::Label::LoadSkill,
                catalog::display::Label::ReadSkillResource,
            ]
            .map(|label| {
                let mut display = catalog::display::new(label);
                display.input = Some(
                    serde_json::from_value(json!({
                        "summary":{"path":"/skill"}
                    }))
                    .expect("skill display is valid"),
                );
                display
            })
        })[usize::from(resource)]
    }

    fn bind(
        resources: Arc<Resources>,
        session: String,
        stop: CancellationToken,
        ingress: Arc<Ingress>,
        turn: sailry_protocol::TurnId,
    ) -> [Arc<Self>; 2] {
        [false, true].map(|resource| {
            Arc::new(Self {
                resources: resources.clone(),
                session: session.clone(),
                stop: stop.clone(),
                resource,
                ingress: ingress.clone(),
                turn,
            })
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Load {
    skill: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Read {
    skill: String,
    path: String,
}

impl Skills {
    fn name(&self) -> &str {
        if self.resource { NAMES[1] } else { NAMES[0] }
    }
    fn description(&self) -> &str {
        if self.resource {
            "Read complete UTF-8 text from a file relative to an available skill's directory. Use package:skill and a normalized relative path. Binary or oversized resources report an error, not a partial file."
        } else {
            "Load the complete SKILL.md for an available package:skill. The execution Node prepares required Python dependencies and returns python beside directory when available, otherwise null. Run Python scripts with \"$SAILRY_PYTHON\" and absolute script paths; the most recently loaded skill with an available Python interpreter selects it for this turn. Read referenced documents with read_skill_resource. Skill loading grants no command permission."
        }
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn parameters_schema(&self) -> Option<Value> {
        Some(if self.resource {
            // load_skill already declares the catalog; the read validates the key.
            json!({"type": "object", "additionalProperties": false, "required": ["skill", "path"], "properties": {
                "skill": {"type": "string", "description": "package:skill key"}, "path": {"type": "string"}
            }})
        } else {
            let keys: Vec<_> = self
                .resources
                .skills()
                .into_iter()
                .map(|(key, _)| key)
                .collect();
            let skill = json!({
                "type": "string",
                "description": "Exact package:skill key from this turn's available skill catalog",
                "enum": keys
            });
            json!({"type": "object", "additionalProperties": false, "required": ["skill"], "properties": {"skill": skill}})
        })
    }
    async fn execute(
        &self,
        context: Arc<CallContext>,
        arguments: Value,
    ) -> Result<rmcp::model::CallToolResult, Fault> {
        if context.session_id() != self.session
            || context.is_cancelled()
            || self.stop.is_cancelled()
        {
            return Err(Fault::invalid("skill invocation is no longer active"));
        }
        let (key, path) = if self.resource {
            let args: Read = serde_json::from_value(arguments)
                .map_err(|_| Fault::invalid("invalid skill resource arguments"))?;
            (args.skill, args.path)
        } else {
            let args: Load = serde_json::from_value(arguments)
                .map_err(|_| Fault::invalid("invalid skill arguments"))?;
            (args.skill, "SKILL.md".into())
        };
        let output = match self.resources.read(&key, &path).await {
            Ok(text) => {
                if self.resource {
                    json!({"skill": key, "path": path, "content": text.content, "directory": text.directory})
                } else {
                    let ready = self
                        .resources
                        .python(&key, &self.ingress.environment)
                        .await
                        .map_err(|error| Fault::invalid(error.to_string()))?;
                    if let Some(ready) = &ready {
                        self.ingress
                            .agents
                            .bind_python(self.turn, ready.clone(), text.directory.clone().into())
                            .map_err(|error| Fault::invalid(error.to_string()))?;
                    }
                    json!({"skill": key, "path": path, "content": text.content, "directory": text.directory,"python":ready.map(|ready| ready.python)})
                }
            }
            Err(error) => json!({"error": error}),
        };
        catalog::json(output)
    }
}

/// Core loaders share the invocation's existing immutable package resources.
pub(super) fn bind(
    resources: Arc<Resources>,
    session: sailry_protocol::SessionId,
    stop: CancellationToken,
    ingress: Arc<Ingress>,
    turn: sailry_protocol::TurnId,
) -> Result<(Vec<catalog::Registration>, String), Fault> {
    let instruction = resources.catalog()?;
    let tools = if resources.skills().is_empty() {
        Vec::new()
    } else {
        Skills::bind(resources.clone(), session.to_string(), stop, ingress, turn)
            .into_iter()
            .enumerate()
            .map(|(index, tool)| {
                let display = (tool.name().into(), Skills::display(index == 1).clone());
                let mut registration = catalog::Registration::managed(tool.descriptor(), tool);
                registration.presentation = Presentation::Summary;
                registration.display = Some(display);
                registration
            })
            .collect()
    };
    Ok((tools, instruction))
}

#[cfg(test)]
mod tests;

crate::agent::catalog::provider!(Skills, crate::agent::catalog::result);
