//! Native code services stay generic; context composition belongs to the package.
use super::*;
use crate::agent::tools::decode;

pub(super) fn handles(operation: Operation) -> bool {
    matches!(operation, Operation::SearchCode | Operation::OutlineCode)
}

pub(super) fn description(operation: Operation) -> &'static str {
    match operation {
        Operation::SearchCode => "Search indexed symbols in the captured worktree",
        Operation::OutlineCode => {
            "Read a file's structure with UTF-8 byte ranges and a full-content revision"
        }
        _ => unreachable!("code operation expected"),
    }
}

pub(super) fn parameters(operation: Operation) -> Value {
    match operation {
        Operation::SearchCode => {
            json!({"type":"object","additionalProperties":false,"properties":{"query":{"type":"string","maxLength":4096},"name":{"type":"string","maxLength":512},"path":{"type":"string"},"language":{"type":"string","maxLength":128},"limit":{"type":"integer","minimum":1,"maximum":100}}})
        }
        Operation::OutlineCode => {
            json!({"type":"object","additionalProperties":false,"properties":{"path":{"type":"string","minLength":1}},"required":["path"]})
        }
        _ => unreachable!("code operation expected"),
    }
}

pub(super) fn command(
    operation: Operation,
    arguments: Value,
    context: &plugin::Context,
) -> Result<Command, Fault> {
    let worktree = context
        .worktree
        .ok_or_else(|| Fault::invalid("code operation requires a worktree"))?;
    match operation {
        Operation::SearchCode => Ok(Command::SearchCode {
            worktree,
            query: decode(arguments)?,
        }),
        Operation::OutlineCode => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Outline {
                path: String,
            }
            let input: Outline = decode(arguments)?;
            Ok(Command::OutlineCode {
                worktree,
                path: input.path,
            })
        }
        _ => Err(Fault::invalid("code operation expected")),
    }
}

pub(super) struct Cancellation {
    queries: Arc<crate::code_index::Queries>,
    caller: sailry_protocol::NodeId,
    request: sailry_protocol::RequestId,
    active: bool,
}

impl Cancellation {
    pub(super) fn new(
        ingress: &Ingress,
        caller: sailry_protocol::NodeId,
        request: &Request,
    ) -> Option<Self> {
        matches!(
            request.command,
            Command::SearchCode { .. } | Command::OutlineCode { .. }
        )
        .then(|| Self {
            queries: ingress.code_queries.clone(),
            caller,
            request: request.id,
            active: true,
        })
    }
    pub(super) fn finish(&mut self) {
        self.active = false;
    }
}

impl Drop for Cancellation {
    fn drop(&mut self) {
        if self.active {
            self.queries.cancel(self.caller, self.request);
        }
    }
}
