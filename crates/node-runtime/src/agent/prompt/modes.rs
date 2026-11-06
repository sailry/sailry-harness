use sailry_protocol::{Fault, SessionConfig, WorkMode, plugin};

/// Coordination policy of the core Flagship mode. Foreground children spawned in
/// one response run together; the parent resumes after all of them finish.
const FLAGSHIP: &str = "You coordinate delegated children. Delegate in parallel whenever the task splits into two or more independent multi-step slices; handle single-step or tightly coupled tasks directly.
1. Do bootstrap and blocking steps yourself: read applicable guides, locate the relevant code and plan. Split the work into slices with disjoint file ownership; keep shared registration files such as module lists, manifests, lockfiles and localization catalogs for yourself.
2. Fan out in waves. Issue every spawn_agent call of a wave in the same response: foreground children from one response run concurrently and you resume after all of them finish, while one spawn per response runs children one after another. Children beyond the concurrency budget in turn context wait in a queue.
3. Delegate multi-area investigation, implementation slices, test writing and review. Keep single lookups, small edits, tightly coupled changes and integration yourself. Never delegate 'read file X and return it'.
4. You own builds and test suites. Children skip them unless their slice is verification. After a wave returns, review the changes, apply shared-file edits, then run one consolidated build and the relevant tests. Send failures back to the owning child through guide_agent or spawn_agent reuse of its session ID, or fix small ones yourself.
5. Brief each child with the goal, owned files, read-only context paths with key excerpts, interfaces to preserve and the expected report. Do not redo a child's work; verify only the claims you rely on.";

pub(in crate::agent) fn workflow(
    packages: &[plugin::Info],
    config: &SessionConfig,
    child: bool,
) -> Result<String, Fault> {
    let selected = crate::plugins::modes::selected(packages, config)?;
    let instructions = if child {
        "Complete the delegated scope and report to the parent. Delegated children cannot create or control other agents.".into()
    } else if let Some((_, mode)) = selected {
        format!(
            "Active work mode: {}\n{}",
            mode.display.label, mode.instructions
        )
    } else if config.mode == WorkMode::Flagship {
        format!("Active work mode: Flagship\n{FLAGSHIP}")
    } else {
        "Handle the task directly. Use subagents only when the user explicitly requests subagents, delegation or parallel agent work. A task's size or complexity alone does not authorize delegation.".into()
    };
    Ok(format!(
        "Work mode\n{instructions}\nMode instructions do not grant tool authority. Follow the captured mode and tool permissions, including read-only restrictions inherited by delegated tasks."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::modes::tests::{config, package};
    use sailry_protocol::Permission;
    use serde_json::json;

    #[test]
    fn preserves_captured_instructions() {
        for (scenario, check) in [
            (
                "selects_one_coordinator_policy",
                (|scenario: &'static str| {
                    let packages = [package("example", "review", false)];
                    let mut config = config();
                    config.options =
                        serde_json::from_value(json!({"delegation":{"proactive":true}})).unwrap();
                    let build = workflow(&packages, &config, false).unwrap();
                    assert!(
                        build.contains("only when the user explicitly requests"),
                        "{scenario}"
                    );
                    assert!(!build.contains(FLAGSHIP), "{scenario}");
                    assert!(
                        super::super::instruction(config.mode, config.permission, false)
                            .contains("Ask permission"),
                        "{scenario}"
                    );
                    config.mode = WorkMode::Plan;
                    let plan = workflow(&packages, &config, false).unwrap();
                    assert_eq!(plan, build, "{scenario}");
                    assert!(
                        super::super::instruction(config.mode, Permission::Full, false)
                            .contains("Do not implement changes or run tools with side effects"),
                        "{scenario}"
                    );
                    config.mode = WorkMode::Flagship;
                    let flagship = workflow(&packages, &config, false).unwrap();
                    assert!(
                        flagship.contains("Active work mode: Flagship"),
                        "{scenario}"
                    );
                    for required in [
                        "You coordinate delegated children",
                        "two or more independent multi-step slices",
                        "disjoint file ownership",
                        "Issue every spawn_agent call of a wave in the same response",
                        "concurrency budget in turn context",
                        "You own builds and test suites",
                        "one consolidated build",
                        "Never delegate 'read file X and return it'",
                    ] {
                        assert!(flagship.contains(required), "{scenario}: {required}");
                    }
                    assert!(
                        !flagship.contains("only when the user explicitly requests"),
                        "{scenario}"
                    );
                    assert_eq!(config.permission, Permission::Ask, "{scenario}");
                    assert!(
                        flagship.contains("do not grant tool authority"),
                        "{scenario}"
                    );
                }) as fn(&'static str),
            ),
            (
                "retired_package_option_is_inert",
                (|scenario: &'static str| {
                    let mut config = config();
                    config.options = serde_json::from_value(
                        json!({"delegation":{"mode":"multitask","proactive":true}}),
                    )
                    .unwrap();
                    let build = workflow(&[], &config, false).unwrap();
                    assert!(
                        build.contains("only when the user explicitly requests"),
                        "{scenario}"
                    );
                    assert!(!build.contains(FLAGSHIP), "{scenario}");
                }),
            ),
            (
                "children_keep_effective_restrictions",
                (|scenario: &'static str| {
                    let mut config = config();
                    config.mode = WorkMode::Flagship;
                    let child = workflow(&[], &config, true).unwrap();
                    assert!(!child.contains(FLAGSHIP), "{scenario}");
                    assert!(
                        child.contains("children cannot create or control other agents"),
                        "{scenario}"
                    );
                    config.mode = WorkMode::Plan;
                    assert!(
                        super::super::instruction(config.mode, Permission::Full, true)
                            .contains("delegated task is read-only"),
                        "{scenario}"
                    );
                }),
            ),
            (
                "package_mode_excludes_flagship",
                (|scenario: &'static str| {
                    let packages = [package("example", "review", true)];
                    let mut config = config();
                    config.mode = WorkMode::Flagship;
                    config.options =
                        serde_json::from_value(json!({"example":{"mode":"review"}})).unwrap();
                    assert!(workflow(&packages, &config, false).is_err(), "{scenario}");
                    config.mode = WorkMode::Code;
                    let selected = workflow(&packages, &config, false).unwrap();
                    assert!(selected.contains("Active work mode: review"), "{scenario}");
                    assert!(!selected.contains(FLAGSHIP), "{scenario}");
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
