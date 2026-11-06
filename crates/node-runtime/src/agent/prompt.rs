use sailry_protocol::{Permission, WorkMode};

mod environment;
pub(in crate::agent) mod instructions;
mod modes;
mod personal;
pub(crate) mod session;
pub(super) use environment::capture as environment;
pub(super) use modes::workflow;
pub(super) use personal::capture as personal;
pub(in crate::agent) use session::Session;

pub(super) fn baseline() -> &'static str {
    include_str!("prompt.txt").trim()
}

// Workflow references: Pi 428a12bc775145afa342530a9eaa652efb3e4422,
// packages/coding-agent/src/core/system-prompt.ts; OpenCode
// f03046d9f558bc54a090363fea48441b05f522bf, packages/opencode/src/session/
// system.ts and prompt/{gpt-astra,codex,default}.txt. Sailry's text is original:
// it preserves Node tool declarations and mode/permission ownership.
#[cfg(test)]
pub(super) fn instruction(mode: WorkMode, permission: Permission, child: bool) -> String {
    format!("{}\n\n{}", baseline(), policy(mode, permission, child))
}

pub(super) fn policy(mode: WorkMode, permission: Permission, child: bool) -> &'static str {
    if mode == WorkMode::Plan && child {
        "This delegated task is read-only. Gather evidence with read-only tools, make no changes and run no tools with side effects, then report your findings."
    } else if mode == WorkMode::Plan {
        "This is a planning turn. Explore with read-only tools and produce a concrete plan for requested multi-step work; answer pure questions directly. Explain the relevant background, recommend one approach, identify key files and reusable existing functions by path, and give implementation steps, verification, risks and open questions. Scale detail to complexity. Do not implement changes or run tools with side effects. An ordinary text or choice answer cannot change work mode."
    } else {
        super::approval::instruction(permission)
    }
}

pub(super) fn recipient(child: bool) -> &'static str {
    if child {
        include_str!("prompt/child.txt").trim()
    } else {
        include_str!("prompt/parent.txt").trim()
    }
}

/// File tools discover scoped guides through the existing captured Node worktree.
pub(in crate::agent) async fn observe(
    ingress: &crate::store::Ingress,
    worktree: sailry_protocol::WorktreeId,
    operation: sailry_protocol::tool::Operation,
    context: &crate::agent::catalog::CallContext,
    output: &mut serde_json::Value,
    stop: &sailry_link::CancellationToken,
) -> Result<(), sailry_protocol::Fault> {
    use instructions::Scope;
    use sailry_protocol::tool::Operation;
    use serde_json::{Value, json};
    if output.get("error").is_some() {
        return Ok(());
    }
    let data = output.get("data").unwrap_or(output);
    let Some(path) = data.get("path").and_then(Value::as_str).map(str::to_owned) else {
        return Ok(());
    };
    let revision = data.get("revision").cloned();
    let scope = match operation {
        Operation::ListDirectory => Scope::Directory(path.clone()),
        Operation::ReadFile | Operation::ReadFileLines | Operation::ReadOffice => {
            Scope::File(path.clone())
        }
        _ => return Ok(()),
    };
    if matches!(operation, Operation::ReadFile | Operation::ReadFileLines)
        && revision.as_ref().is_some_and(|revision| {
            context
                .context
                .guides
                .get(&instructions::key(&path))
                .is_some_and(|guide| guide.path == path && json!(guide.revision) == *revision)
        })
    {
        output["sailry_guide_loaded"] = json!({"path":path,"revision":revision});
    }
    let mut delta = crate::agent::canonical::Context::default();
    for guide in instructions::capture(ingress, worktree, &[scope], stop).await? {
        if context
            .context
            .guides
            .get(&guide.key())
            .is_none_or(|previous| previous.revision != guide.revision)
        {
            delta.guides.insert(
                guide.key(),
                crate::agent::canonical::Guide {
                    path: guide.path.clone(),
                    revision: guide.revision.clone(),
                    content: guide.render(),
                },
            );
        }
    }
    if !delta.guides.is_empty() {
        output["sailry_context"] = serde_json::to_value(delta)
            .map_err(|_| sailry_protocol::Fault::invalid("captured guides could not be encoded"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::approval;
    use super::*;
    use std::collections::BTreeSet;

    const PERMISSIONS: [Permission; 3] = [Permission::Ask, Permission::Project, Permission::Full];

    /// The first line of each blank-line separated block.
    fn headings(text: &str) -> Vec<&str> {
        text.split("\n\n")
            .filter_map(|block| block.lines().next())
            .collect()
    }

    fn lines(text: &str) -> BTreeSet<&str> {
        text.lines().filter(|line| !line.is_empty()).collect()
    }

    #[test]
    fn baseline_holds_common_workflow() {
        let baseline = baseline();
        assert_eq!(
            &headings(baseline)[1..],
            ["Work", "Tools and context", "Progress and verification"]
        );
        for tool in [
            "code_search",
            "code_context",
            "search_files",
            "code_outline",
        ] {
            assert!(baseline.contains(tool), "{tool}");
        }
        // Optional and recipient-specific tools belong to their own declarations.
        for tool in [
            "ask_user",
            "list_sessions",
            "update_plan",
            "set_session_title",
        ] {
            assert!(!baseline.contains(tool), "{tool}");
        }
        for operation in [
            "git reset --hard",
            "git checkout --",
            "git clean",
            "force push",
            "--no-verify",
        ] {
            assert!(baseline.contains(operation), "{operation}");
        }
    }

    #[test]
    fn separates_recipient_rules() {
        let (parent, child) = (recipient(false), recipient(true));
        assert_eq!(
            headings(parent),
            ["Responses", "Files and previews", "Charts and formulas"]
        );
        assert_eq!(headings(child), ["Delegated child"]);
        for (left, right) in [(baseline(), parent), (baseline(), child), (parent, child)] {
            assert!(lines(left).is_disjoint(&lines(right)));
        }
        // Only the user-facing recipient asks the user, resumes sessions or renders charts.
        for capability in ["ask_user", "list_sessions", "```chart"] {
            assert!(parent.contains(capability), "{capability}");
            assert!(!child.contains(capability), "{capability}");
        }
        assert!(parent.contains("Finish with an answer to the user's request"));
        for rule in [
            "returns to the parent agent, not the user",
            "Do not ask the user for input",
            "does not provide new user consent",
            "unless the delegated task explicitly assigns verification",
        ] {
            assert!(child.contains(rule), "{rule}");
        }
    }

    #[test]
    fn planning_ignores_permission() {
        let prompt = instruction(WorkMode::Plan, Permission::Ask, false);
        for permission in PERMISSIONS {
            assert_eq!(instruction(WorkMode::Plan, permission, false), prompt);
            assert!(!prompt.contains(approval::instruction(permission)));
        }
        assert!(prompt.contains("Do not implement changes or run tools with side effects"));
        assert!(prompt.contains("cannot change work mode"));
    }

    #[test]
    fn delegated_planning_reports_findings() {
        let prompt = instruction(WorkMode::Plan, Permission::Full, true);
        assert!(prompt.contains("delegated task is read-only"));
        assert!(prompt.contains("report your findings"));
        assert!(!prompt.contains(policy(WorkMode::Plan, Permission::Full, false)));
        for permission in PERMISSIONS {
            assert!(!prompt.contains(approval::instruction(permission)));
            assert_eq!(
                instruction(WorkMode::Code, permission, true),
                instruction(WorkMode::Code, permission, false)
            );
        }
    }

    #[test]
    fn execution_selects_admitted_permission() {
        for permission in PERMISSIONS {
            let prompt = instruction(WorkMode::Code, permission, false);
            assert!(prompt.starts_with(baseline()));
            assert!(prompt.ends_with(approval::instruction(permission)));
            assert!(!prompt.contains(policy(WorkMode::Plan, permission, false)));
            for other in PERMISSIONS.into_iter().filter(|other| *other != permission) {
                assert!(!prompt.contains(approval::instruction(other)));
            }
        }
    }
}
