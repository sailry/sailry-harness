use super::*;
use adk_core::{ToolConfirmationDecision, ToolConfirmationHandler, ToolConfirmationRequest};
use async_trait::async_trait;

pub(super) fn instruction(permission: sailry_protocol::Permission) -> &'static str {
    match permission {
        sailry_protocol::Permission::Ask => {
            "This turn uses Ask permission. Invoke tools directly for the user's task; the runtime pauses file writes, shell commands, worktree changes and non-read-only MCP tools for approval before execution. Do not ask for that approval through ask_user or prose. Read-only tools run without approval. MCP annotations describe the configured server's behavior; they are not an OS sandbox."
        }
        sailry_protocol::Permission::Project => {
            "This turn uses Project permission. Writes through project file tools run without individual approval. Invoke shell commands, worktree changes and non-read-only MCP tools directly for the user's task; the runtime pauses them for approval before execution. Do not ask for that approval through ask_user or prose. Their effects cannot be confined by their working directory; there is no OS sandbox. Ordinary read-only tools run without approval."
        }
        sailry_protocol::Permission::Full => {
            "This turn has full permission for available tools under the execution Node user's OS access. File writes, shell commands and worktree changes do not require individual approval. Follow the user's intent and do not infer that unrelated destructive actions are authorized."
        }
    }
}

pub(super) struct Handler {
    pub ingress: Arc<Ingress>,
    pub turn: TurnId,
    pub stop: CancellationToken,
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
            result = self.ingress.confirm(self.turn, request.clone()) => {
                match result.map_err(|error| adk_core::AdkError::tool(error.to_string()))? {
                    sailry_protocol::conversation::Decision::Approve => Ok(ToolConfirmationDecision::Approve),
                    sailry_protocol::conversation::Decision::Deny => Ok(ToolConfirmationDecision::Deny),
                }
            }
        }
    }
}
