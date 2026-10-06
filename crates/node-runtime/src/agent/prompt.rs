use sailry_protocol::{Permission, WorkMode};

// Workflow references: Pi 428a12bc775145afa342530a9eaa652efb3e4422,
// packages/coding-agent/src/core/system-prompt.ts; OpenCode
// f03046d9f558bc54a090363fea48441b05f522bf, packages/opencode/src/session/
// system.ts and prompt/{gpt-astra,codex,default}.txt. Sailry's text is original:
// it preserves Node tool declarations and mode/permission ownership.
pub(super) fn instruction(mode: WorkMode, permission: Permission) -> String {
    let mode = if mode == WorkMode::Plan {
        "This is a planning turn. Explore with read-only tools and produce a concrete plan; do not implement changes or run tools with side effects. If ask_user offers input kind plan, use it to present the completed plan for review. Its dedicated response can start a new coding turn; an ordinary text or choice answer cannot change work mode. Otherwise return the plan as text."
    } else {
        super::approval::instruction(permission)
    };
    format!("{}\n\n{mode}", include_str!("prompt.txt").trim())
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
        assert!(prompt.contains("do not implement changes or run tools with side effects"));
        assert!(prompt.contains("ordinary text or choice answer cannot change work mode"));
        assert!(!prompt.contains("This turn uses Ask permission"));
        assert!(!prompt.contains("This turn uses Project permission"));
        assert!(!prompt.contains("This turn has full permission"));
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
        }
    }
}
