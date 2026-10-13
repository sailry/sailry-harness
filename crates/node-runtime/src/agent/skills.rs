//! Standard skill loading uses ADK tools and the existing turn/session history.
use super::*;
use crate::plugins::resources::Resources;
use adk_core::{AdkError, Tool, ToolContext};
use async_trait::async_trait;
use sailry_protocol::tool::{Display, Field, Input, Presentation};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::OnceLock};
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
            #[derive(Deserialize)]
            struct Labels {
                #[serde(rename = "tool_load_skill")]
                load: String,
                #[serde(rename = "tool_read_skill_resource")]
                read: String,
            }
            let labels = [
                ("en", include_str!("../../../../locales/en.yml")),
                ("zh-CN", include_str!("../../../../locales/zh-CN.yml")),
                (
                    "zh-TW",
                    include_str!("../../../../locales/application.zh-TW.yml"),
                ),
                ("ja", include_str!("../../../../locales/application.ja.yml")),
                ("ko", include_str!("../../../../locales/application.ko.yml")),
                ("fr", include_str!("../../../../locales/application.fr.yml")),
                ("de", include_str!("../../../../locales/application.de.yml")),
                ("es", include_str!("../../../../locales/application.es.yml")),
                (
                    "pt-BR",
                    include_str!("../../../../locales/application.pt-BR.yml"),
                ),
                ("ru", include_str!("../../../../locales/application.ru.yml")),
            ]
            .map(|(locale, source)| {
                let labels: Labels =
                    serde_saphyr::from_str(source).expect("skill labels are valid");
                (locale, [labels.load, labels.read])
            });
            [0, 1].map(|index| Display {
                label: labels[0].1[index].clone(),
                icon: None,
                locales: labels[1..]
                    .iter()
                    .map(|(locale, values)| ((*locale).into(), values[index].clone()))
                    .collect::<BTreeMap<_, _>>(),
                input: Some(Input {
                    summary: Some(Field {
                        path: "/skill".into(),
                        code: false,
                        omit: Vec::new(),
                        format: None,
                        result_path: None,
                        names: BTreeMap::new(),
                    }),
                    context: None,
                    target: None,
                    content: None,
                }),
                output: None,
                approval: Vec::new(),
                aliases: Vec::new(),
            })
        })[usize::from(resource)]
    }

    fn bind(
        resources: Arc<Resources>,
        session: String,
        stop: CancellationToken,
        ingress: Arc<Ingress>,
        turn: sailry_protocol::TurnId,
    ) -> [Arc<dyn Tool>; 2] {
        [false, true].map(|resource| {
            Arc::new(Self {
                resources: resources.clone(),
                session: session.clone(),
                stop: stop.clone(),
                resource,
                ingress: ingress.clone(),
                turn,
            }) as Arc<dyn Tool>
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

#[async_trait]
impl Tool for Skills {
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
    fn is_concurrency_safe(&self) -> bool {
        self.resource
    }
    fn parameters_schema(&self) -> Option<Value> {
        Some(if self.resource {
            json!({"type": "object", "additionalProperties": false, "required": ["skill", "path"], "properties": {"skill": {"type": "string"}, "path": {"type": "string"}}})
        } else {
            json!({"type": "object", "additionalProperties": false, "required": ["skill"], "properties": {"skill": {"type": "string"}}})
        })
    }
    async fn execute(
        &self,
        context: Arc<dyn ToolContext>,
        arguments: Value,
    ) -> adk_core::Result<Value> {
        if context.session_id() != self.session
            || context.is_cancelled()
            || self.stop.is_cancelled()
        {
            return Err(AdkError::tool("skill invocation is no longer active"));
        }
        let (key, path) = if self.resource {
            let args: Read = serde_json::from_value(arguments)
                .map_err(|_| AdkError::tool("invalid skill resource arguments"))?;
            (args.skill, args.path)
        } else {
            let args: Load = serde_json::from_value(arguments)
                .map_err(|_| AdkError::tool("invalid skill arguments"))?;
            (args.skill, "SKILL.md".into())
        };
        Ok(match self.resources.read(&key, &path).await {
            Ok(text) => {
                if self.resource {
                    json!({"skill": key, "path": path, "content": text.content, "directory": text.directory})
                } else {
                    let ready = self
                        .resources
                        .python(&key, &self.ingress.environment)
                        .await
                        .map_err(|error| AdkError::tool(error.to_string()))?;
                    if let Some(ready) = &ready {
                        self.ingress
                            .agents
                            .bind_python(self.turn, ready.clone(), text.directory.clone().into())
                            .map_err(|error| AdkError::tool(error.to_string()))?;
                    }
                    json!({"skill": key, "path": path, "content": text.content, "directory": text.directory,"python":ready.map(|ready| ready.python)})
                }
            }
            Err(error) => json!({"error": error}),
        })
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
                let mut registration = catalog::Registration::managed(tool);
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
