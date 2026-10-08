use super::*;
use crate::store::agent::ApprovalBinding;
use adk_core::{ToolConfirmationDecision, ToolConfirmationHandler, ToolConfirmationRequest};
use async_trait::async_trait;
use std::collections::BTreeMap;

mod denials;
pub(super) use denials::bind;

pub(super) fn bindings(
    invocation: &Invocation,
    tools: &[catalog::Registration],
) -> Result<BTreeMap<String, ApprovalBinding>, Fault> {
    let mut bindings = BTreeMap::new();
    for registration in tools {
        let Some(owner) = &registration.plugin else {
            continue;
        };
        let package = invocation
            .plugins
            .iter()
            .find(|package| &package.summary.name == owner)
            .ok_or_else(|| {
                Fault::invalid("registered tool owner is outside the captured plugins")
            })?;
        let name = registration.tool.name().to_owned();
        bindings.insert(
            name.clone(),
            ApprovalBinding {
                owner: package.summary.reference(),
                operation: crate::plugins::tools::operation(package, &name),
                tool: name,
            },
        );
    }
    Ok(bindings)
}

pub(super) fn instruction(permission: sailry_protocol::Permission) -> &'static str {
    match permission {
        sailry_protocol::Permission::Ask => {
            "This turn uses Ask permission. Invoke tools directly for the user's task; the runtime pauses file writes, shell commands, worktree changes and non-read-only MCP tools for approval before execution unless a matching user-approved rule applies in this session. Do not request those tool approvals through a question tool or prose. Prior task authorization does not bypass runtime approval. Prefer dedicated read-only tools for exploration; they run without approval. MCP annotations describe the configured server's behavior; they are not an OS sandbox."
        }
        sailry_protocol::Permission::Project => {
            "This turn uses Project permission. Writes through project file tools run without individual approval. Invoke shell commands, worktree changes and non-read-only MCP tools directly for the user's task; the runtime pauses them for approval before execution unless a matching user-approved rule applies in this session. Do not request those tool approvals through a question tool or prose. Prior task authorization does not bypass runtime approval. Their effects cannot be confined by their working directory; there is no OS sandbox. Prefer dedicated read-only tools for exploration; they run without approval."
        }
        sailry_protocol::Permission::Full => {
            "This turn has full permission for available tools under the OS access of the machine running this session. File writes, shell commands and worktree changes do not require individual approval. Follow the user's intent and do not infer that unrelated destructive actions are authorized."
        }
    }
}

pub(super) struct Handler {
    pub ingress: Arc<Ingress>,
    pub turn: TurnId,
    pub stop: CancellationToken,
    pub bindings: BTreeMap<String, ApprovalBinding>,
}

impl std::fmt::Debug for Handler {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApprovalHandler")
            .field("turn", &self.turn)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl ToolConfirmationHandler for Handler {
    async fn decide(
        &self,
        request: &ToolConfirmationRequest,
    ) -> adk_core::Result<ToolConfirmationDecision> {
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => Err(adk_core::AdkError::tool("tool invocation was cancelled")),
            result = self.ingress.confirm(self.turn, request.clone(), self.bindings.get(&request.tool_name).cloned()) => {
                match result.map_err(|error| adk_core::AdkError::tool(error.to_string()))? {
                    sailry_protocol::conversation::Decision::Approve
                    | sailry_protocol::conversation::Decision::ApproveSession
                    | sailry_protocol::conversation::Decision::ApprovePrefix(_) => Ok(ToolConfirmationDecision::Approve),
                    sailry_protocol::conversation::Decision::Deny
                    | sailry_protocol::conversation::Decision::DenyWithReason(_) => Ok(ToolConfirmationDecision::Deny),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_explicit_session_rules() {
        for permission in [
            sailry_protocol::Permission::Ask,
            sailry_protocol::Permission::Project,
        ] {
            assert!(
                instruction(permission)
                    .contains("unless a matching user-approved rule applies in this session")
            );
            assert!(
                instruction(permission)
                    .contains("Prior task authorization does not bypass runtime approval")
            );
        }
    }
}
