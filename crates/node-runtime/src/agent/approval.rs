use super::*;
use crate::store::agent::ApprovalBinding;
use std::collections::BTreeMap;

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
        let name = registration.tool.name.to_string();
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
            "This turn uses Ask permission. Invoke tools directly for the user's task; the runtime pauses file writes, worktree changes and non-read-only MCP tools for approval before execution unless a matching user-approved rule applies in this session. Do not request those tool approvals through a question tool or prose. Prior task authorization does not bypass runtime approval. Prefer dedicated read-only tools for exploration; they run without approval. On macOS/Linux, when the command sandbox is enabled, workspace commands run without approval inside a worktree sandbox with no network. Unrestricted commands require fresh user approval. Windows retains command approval by permission mode."
        }
        sailry_protocol::Permission::Project => {
            "This turn uses Project permission. Writes through project file tools run without individual approval. Invoke worktree changes and non-read-only MCP tools directly for the user's task; the runtime pauses them for approval before execution unless a matching user-approved rule applies in this session. Do not request those tool approvals through a question tool or prose. Prior task authorization does not bypass runtime approval. Prefer dedicated read-only tools for exploration; they run without approval. On macOS/Linux, when the command sandbox is enabled, workspace commands run without approval inside a worktree sandbox with no network. Unrestricted commands require fresh user approval. Windows retains command approval by permission mode."
        }
        sailry_protocol::Permission::Full => {
            "This turn has full permission for available tools under the OS access of the machine running this session. File writes and worktree changes do not require individual approval. On macOS/Linux, when the command sandbox is enabled, workspace commands use a worktree sandbox with no network. Request unrestricted access when a command needs network or files outside the worktree; these commands run without individual approval under Full permission. Windows retains ordinary command execution under Full permission. Follow the user's intent and do not infer that unrelated destructive actions are authorized."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_command_access_by_permission() {
        for permission in [
            sailry_protocol::Permission::Ask,
            sailry_protocol::Permission::Project,
            sailry_protocol::Permission::Full,
        ] {
            let text = instruction(permission);
            assert!(text.contains("worktree sandbox with no network"));
            assert!(text.contains("Windows retains"));
            assert!(!text.contains("there is no OS sandbox"));
            assert!(!text.contains("command sandbox setting"));
            if permission == sailry_protocol::Permission::Full {
                assert!(text.contains("network or files outside the worktree"));
                assert!(text.contains("commands run without individual approval"));
                assert!(!text.contains("fresh user approval"));
            } else {
                assert!(text.contains("Unrestricted commands require fresh user approval"));
            }
        }
    }

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
