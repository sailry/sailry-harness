//! Standard skill loading uses ADK tools and the existing turn/session history.
use super::*;
use crate::plugins::resources::Resources;
use adk_core::{AdkError, Tool, ToolContext};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
pub(super) mod management;

pub(super) const NAMES: [&str; 2] = ["load_skill", "read_skill_resource"];

struct Skills {
    resources: Arc<Resources>,
    session: String,
    stop: CancellationToken,
    resource: bool,
}

impl Skills {
    fn bind(
        resources: Arc<Resources>,
        session: String,
        stop: CancellationToken,
    ) -> [Arc<dyn Tool>; 2] {
        [false, true].map(|resource| {
            Arc::new(Self {
                resources: resources.clone(),
                session: session.clone(),
                stop: stop.clone(),
                resource,
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
            "Load the complete SKILL.md for an available package:skill. Read referenced documents with read_skill_resource. Resolve relative script paths against the returned execution Node directory, not the session worktree. Quote script paths when calling the command tool."
        }
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
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
                json!({"skill": key, "path": path, "content": text.content, "directory": text.directory})
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
) -> Result<(Vec<catalog::Registration>, String), Fault> {
    let instruction = resources.catalog()?;
    let tools = if resources.skills().is_empty() {
        Vec::new()
    } else {
        Skills::bind(resources.clone(), session.to_string(), stop)
            .into_iter()
            .map(catalog::Registration::managed)
            .collect()
    };
    Ok((tools, instruction))
}

#[cfg(test)]
mod tests;
