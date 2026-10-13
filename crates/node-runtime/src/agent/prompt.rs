use sailry_protocol::{Permission, WorkMode};

mod environment;
mod instructions;
mod personal;
mod role;
pub(super) use environment::capture as environment;
pub(super) use personal::capture as personal;
pub(super) use role::bind;

// Workflow references: Pi 428a12bc775145afa342530a9eaa652efb3e4422,
// packages/coding-agent/src/core/system-prompt.ts; OpenCode
// f03046d9f558bc54a090363fea48441b05f522bf, packages/opencode/src/session/
// system.ts and prompt/{gpt-astra,codex,default}.txt. Sailry's text is original:
// it preserves Node tool declarations and mode/permission ownership.
pub(super) fn instruction(mode: WorkMode, permission: Permission) -> String {
    let mode = if mode == WorkMode::Plan {
        "This is a planning turn. Explore with read-only tools and produce a concrete plan for requested multi-step work; answer pure questions directly. Explain the relevant background, recommend one approach, identify key files and reusable existing functions by path, and give implementation steps, verification, risks and open questions. Scale detail to complexity. Do not implement changes or run tools with side effects. An ordinary text or choice answer cannot change work mode."
    } else {
        super::approval::instruction(permission)
    };
    format!("{}\n\n{mode}", include_str!("prompt.txt").trim())
}

pub(super) fn recipient(child: bool) -> &'static str {
    if child {
        include_str!("prompt/child.txt").trim()
    } else {
        include_str!("prompt/parent.txt").trim()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planning_preserves_read_only_scope() {
        let prompt = instruction(WorkMode::Plan, Permission::Ask);
        for permission in [Permission::Project, Permission::Full] {
            assert_eq!(instruction(WorkMode::Plan, permission), prompt);
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
    fn execution_selects_admitted_permission() {
        let base = include_str!("prompt.txt").trim();
        for permission in [Permission::Ask, Permission::Project, Permission::Full] {
            let prompt = instruction(WorkMode::Code, permission);
            assert_eq!(
                prompt,
                format!(
                    "{base}\n\n{}",
                    super::super::approval::instruction(permission)
                )
            );
            assert!(!prompt.contains("This is a planning turn"));
            assert!(
                prompt.contains("subsequent commands in every session use unrestricted access")
            );
            assert!(prompt.contains("normal environment"));
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
        let prompt = instruction(WorkMode::Code, Permission::Ask);
        for absent in [
            "admitted roster",
            "update_plan",
            "set_session_title",
            "file card",
            "Finish with an answer",
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
            "Prefer a few concise options and bundle related questions",
            "Continue independent work while waiting",
            "Treat a new user message as steering the current goal",
            "explore with dedicated read-only tools and combine related checks",
            "Finish the cohesive authorized changes before starting costly builds or tests",
            "Run checks directly relevant to changed behavior",
            "broaden only for a concrete risk, failure or uncovered requirement",
            "do not add tests that merely mirror a low-impact implementation",
            "Prefer available find_files and search_files tools",
            "code_symbols, code_definition and code_references",
            "Use rg through a command tool only as a supplement",
            "Use git --no-pager",
            "Avoid interactive prompts or editors",
            "do not print large files through scripts or add echo/printf separators",
            "backticks and $() in command text execute substitutions",
            "JSON encoding is not shell escaping",
            "Avoid a blocking wait longer than 60 seconds",
            "Project guides shown in this turn are already loaded",
            "Do not reread them with a shell",
            "prefer available search tools and read-only URL retrieval",
            "Use the browser for interaction, login, pages that require JavaScript",
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
        assert!(!child.contains("Finish with an answer to the user's request"));
        assert!(!child.contains("renders a file card"));
        assert!(recipient(false).contains("Finish with an answer to the user's request"));
        assert!(recipient(false).contains("at most six bullets"));
        for rule in [
            "1. Create or update the real file with available tools and verify the result",
            "2. Put EACH deliverable's Markdown link alone in its own paragraph",
            "blank lines before and after it",
            "[output/report.docx](output/report.docx)",
            "Do not add a prefix, suffix, list marker, code fence or blockquote",
            "3. Put explanations in separate paragraphs",
            "Preserve exact returned paths and filenames; never invent a file path",
            "Ordinary source references may remain inline links",
            "Ordinary links within prose remain links and are not added to outputs",
            "No display or publish tool is needed; do not print internal content JSON",
            "with a local copy prepared first for remote files",
        ] {
            assert!(recipient(false).contains(rule), "{rule}");
            assert!(!child.contains(rule), "{rule}");
        }
    }
}
