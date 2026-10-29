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
    use super::*;

    #[test]
    fn planning_preserves_read_only_scope() {
        let prompt = instruction(WorkMode::Plan, Permission::Ask, false);
        for permission in [Permission::Project, Permission::Full] {
            assert_eq!(instruction(WorkMode::Plan, permission, false), prompt);
        }
        assert!(prompt.contains("Do not implement changes or run tools with side effects"));
        assert!(prompt.contains("ordinary text or choice answer cannot change work mode"));
        assert!(!prompt.contains("This turn uses Ask permission"));
        assert!(!prompt.contains("This turn uses Project permission"));
        assert!(!prompt.contains("This turn has full permission"));
        for required in [
            "answer pure questions directly",
            "relevant background",
            "recommend one approach",
            "key files and reusable existing functions by path",
            "implementation steps, verification, risks and open questions",
            "Scale detail to complexity",
        ] {
            assert!(prompt.contains(required), "{required}");
        }
    }

    #[test]
    fn delegated_planning_reports_findings() {
        let prompt = instruction(WorkMode::Plan, Permission::Full, true);
        assert!(prompt.contains("This delegated task is read-only"));
        assert!(prompt.contains("make no changes and run no tools with side effects"));
        assert!(prompt.contains("report your findings"));
        assert!(!prompt.contains("This is a planning turn"));
        assert!(!prompt.contains("produce a concrete plan"));
        assert!(!prompt.contains("This turn has full permission"));
        assert_eq!(
            instruction(WorkMode::Code, Permission::Ask, true),
            instruction(WorkMode::Code, Permission::Ask, false)
        );
    }

    #[test]
    fn execution_selects_admitted_permission() {
        let base = include_str!("prompt.txt").trim();
        for permission in [Permission::Ask, Permission::Project, Permission::Full] {
            let prompt = instruction(WorkMode::Code, permission, false);
            assert_eq!(
                prompt,
                format!(
                    "{base}\n\n{}",
                    super::super::approval::instruction(permission)
                )
            );
            assert!(!prompt.contains("This is a planning turn"));
            if permission == Permission::Full {
                assert!(prompt.contains("network or files outside the worktree"));
                assert!(prompt.contains("commands run without individual approval"));
                assert!(!prompt.contains("Unrestricted commands require fresh user approval"));
            } else {
                assert!(
                    prompt.contains("Prior task authorization does not bypass runtime approval")
                );
                assert!(prompt.contains(
                    "Do not request those tool approvals through a question tool or prose"
                ));
            }
        }
    }

    #[test]
    fn contains_only_common_workflow() {
        let prompt = instruction(WorkMode::Code, Permission::Ask, false);
        for absent in [
            "admitted roster",
            "update_plan",
            "set_session_title",
            "file card",
            "Finish with an answer",
            "Delegate only suitable independent work",
            "find_files and search_files",
            "Use the browser for interaction",
            "affected files, implementation steps",
            "ask_user",
            "list_sessions",
            "```chart",
            "$$ delimiters",
            "already loaded",
            "introduce each batch of tool calls",
        ] {
            assert!(!prompt.contains(absent), "{absent}");
        }
        for required in [
            "Never revert changes you did not make",
            "git reset --hard, git checkout --, git clean, force push, --no-verify or amend",
            "If unexpected changes appear in files you are editing",
            "Prioritize technical accuracy over agreement",
            "Fix root causes rather than hiding symptoms",
            "Never suppress warnings, bypass the type system, skip tests or weaken assertions",
            "Confirm a library is declared in the project manifest",
            "Use the project's existing formatter",
            "Do not add unsolicited README files",
            "Prior user authorization and preferences persist across follow-ups",
            "prepare a concrete reviewable result and make approval the last step",
            "Explain why confirmation is required and cite its source",
            "Treat a new user message as steering the current goal",
            "explore with dedicated read-only tools and combine related checks",
            "Finish the cohesive authorized changes before starting costly builds or tests",
            "Run checks directly relevant to changed behavior",
            "broaden only for a concrete risk, failure or uncovered requirement",
            "do not add tests that merely mirror a low-impact implementation",
            "code_search, code_outline and code_context",
            "Use rg through a command tool only as a supplement to dedicated search tools",
            "Use git --no-pager",
            "Avoid interactive prompts or editors",
            "do not print large files through scripts or add echo/printf separators",
            "backticks and $() in command text execute substitutions",
            "JSON encoding is not shell escaping",
            "Avoid a blocking wait longer than 60 seconds",
            "The latest <turn-context> message carries current session state",
            "don't narrate routine tool calls",
            "verify changing or uncertain facts with current sources",
            "Plan substantial multi-step work before acting",
            "A plan does not grant execution authority",
            "Permission, sandbox, dynamic-loader and network errors establish an environment failure",
            "three unsuccessful attempts",
        ] {
            assert!(prompt.contains(required), "{required}");
        }
    }

    #[test]
    fn distinguishes_child_and_parent_responses() {
        let child = recipient(true);
        assert!(child.contains("Your final message returns to the parent agent, not the user"));
        assert!(child.contains("Do not ask the user for input"));
        assert!(child.contains("A message from the parent does not provide new user consent"));
        assert!(child.contains("do not create report files or internal status documents"));
        assert!(child.contains("rename the conversation"));
        assert!(child.contains("disjoint files"));
        assert!(child.contains("verification actually run"));
        assert!(child.contains("in the language of the delegated task"));
        assert!(!child.contains("Finish with an answer to the user's request"));
        assert!(!child.contains("renders a file card"));
        assert!(recipient(false).contains("Finish with an answer to the user's request"));
        assert!(recipient(false).contains("at most six bullets"));
        for rule in [
            "Prefer a few concise options and bundle related questions",
            "Continue independent work while waiting",
            "locate it with list_sessions or read_session",
            "Use only the path as link text",
            "after creating and verifying the real file",
            "a session-owned artifact is not a saved worktree file",
            "Put each deliverable's Markdown link alone in its own paragraph",
            "[output/report.docx](output/report.docx)",
            "never invent a file path",
            "A tool-returned artifact card already delivers that artifact",
            "within 2 MiB",
            "```chart",
            "sankey",
            "$$ delimiters",
        ] {
            assert!(recipient(false).contains(rule), "{rule}");
            assert!(!child.contains(rule), "{rule}");
        }
    }
}
