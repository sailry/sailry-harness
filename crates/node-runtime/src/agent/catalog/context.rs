//! A tool call uses one immutable Node identity and captured session context.
use super::*;
use crate::agent::canonical::{Context, ToolIdentity};
use goose_agent::machine::MachineSession;
use rmcp::model::CallToolRequestParams;

pub(in crate::agent) trait CatalogSession: MachineSession {
    fn context(&self) -> &Context;
}

#[derive(Clone)]
pub(in crate::agent) struct CallContext {
    session: String,
    request: ToolIdentity,
    stop: CancellationToken,
    pub context: Context,
    pub(super) children: Option<crate::agent::engine::Children>,
}

impl CallContext {
    pub(in crate::agent) fn new(
        session: String,
        id: &str,
        call: &CallToolRequestParams,
        stop: CancellationToken,
        context: Context,
    ) -> Result<Self, rmcp::model::ErrorData> {
        if id.is_empty() {
            return Err(rmcp::model::ErrorData::invalid_request(
                "tool call identity is missing",
                None,
            ));
        }
        Ok(Self {
            session,
            stop,
            context,
            children: None,
            request: ToolIdentity {
                id: Some(id.to_owned()),
                name: call.name.to_string(),
                arguments: serde_json::Value::Object(call.arguments.clone().unwrap_or_default()),
            },
        })
    }

    pub(in crate::agent) fn session_id(&self) -> &str {
        &self.session
    }
    pub(in crate::agent) fn branch(&self) -> &str {
        ""
    }
    pub(in crate::agent) fn function_call_id(&self) -> &str {
        self.request
            .id
            .as_deref()
            .expect("new tool calls have an identity")
    }
    pub(in crate::agent) fn is_cancelled(&self) -> bool {
        self.stop.is_cancelled()
    }
    pub(in crate::agent) fn request(&self) -> ToolIdentity {
        self.request.clone()
    }
    pub(in crate::agent) fn request_with(&self, arguments: Value) -> ToolIdentity {
        ToolIdentity {
            arguments,
            ..self.request()
        }
    }
    pub(in crate::agent) fn children(&self) -> Option<&crate::agent::engine::Children> {
        self.children.as_ref()
    }
}
