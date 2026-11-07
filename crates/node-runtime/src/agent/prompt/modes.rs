use sailry_protocol::{Fault, SessionConfig, WorkMode, plugin};

/// Coordination strategy of the core Flagship mode. Spawn mechanics and briefing
/// belong to the spawn_agent description, which every mode shares.
const FLAGSHIP: &str = "You coordinate delegated children. Delegate whenever the task splits into two or more independent multi-step slices.
1. Do bootstrap and blocking steps yourself: follow the loaded project guides, locate the relevant code and plan. Split the work into slices with disjoint file ownership; keep shared registration files such as module lists, manifests, lockfiles and localization catalogs for yourself.
2. Fan out in waves: issue every spawn_agent call of a wave in one response. Children beyond the concurrency budget in turn context queue.
3. Delegate multi-area investigation, implementation slices, test writing and review. Keep single lookups (never delegate 'read file X and return it'), small edits, tightly coupled changes and integration yourself.
4. Give each slice the configured role whose description matches it and follow any team instructions. Read-only roles run in Plan mode and only gather evidence; give implementation to a writable role or omit role to inherit this turn.
5. You own builds and test suites; give verification to at most one child at a time. After a wave returns, review its changes, apply shared-file edits, then run one consolidated build and the relevant tests. Return failures to the owning child by its session ID, or fix small ones yourself.
6. Do not redo a child's work; verify only the claims you rely on.";

/// Default coordination: direct work, with role-matched delegation when it pays off.
const DIRECT: &str = "Handle the task directly, including single lookups, small edits and tightly coupled changes. Delegate when the user requests subagents, delegation or parallel agent work, when the team instructions call for it, or when an independent multi-step slice matches a configured role's description; give that slice the matching role. A task's size or complexity alone does not authorize delegation.";

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
        DIRECT.into()
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
                    assert!(build.contains(DIRECT), "{scenario}");
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
                    assert!(
                        flagship.contains("You coordinate delegated children"),
                        "{scenario}"
                    );
                    let budget = crate::agent::delegation::tools::BUDGET.to_lowercase();
                    assert!(FLAGSHIP.contains(&budget), "{scenario}");
                    assert!(!flagship.contains(DIRECT), "{scenario}");
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
                    assert!(build.contains(DIRECT), "{scenario}");
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
