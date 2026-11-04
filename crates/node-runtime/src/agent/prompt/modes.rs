use sailry_protocol::{Fault, SessionConfig, WorkMode, plugin};

/// Coordination policy of the core Flagship mode; delegation stays optional.
const FLAGSHIP: &str = "Flagship enables delegation; it does not require it. First plan the task and do the immediate blocking step yourself. Do directed work yourself: reading or searching known files, single lookups, small edits, verifying a child's claim, and anything your next step depends on. Delegate only bounded work that runs in parallel with your own progress: broad multi-area investigation, independent implementation slices with disjoint files, or review and verification that can run alongside. Never delegate 'read file X and return it'. Keep tightly coupled work with one owner. Wait only when blocked; do not redo a child's work.";

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
                    assert!(!build.contains("Flagship enables delegation"), "{scenario}");
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
                        flagship.contains("Flagship enables delegation; it does not require it"),
                        "{scenario}"
                    );
                    assert!(
                        flagship.contains("immediate blocking step yourself"),
                        "{scenario}"
                    );
                    assert!(flagship.contains("disjoint files"), "{scenario}");
                    assert!(
                        flagship.contains("Never delegate 'read file X and return it'"),
                        "{scenario}"
                    );
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
                    assert!(!build.contains("Flagship enables delegation"), "{scenario}");
                }),
            ),
            (
                "children_keep_effective_restrictions",
                (|scenario: &'static str| {
                    let mut config = config();
                    config.mode = WorkMode::Flagship;
                    let child = workflow(&[], &config, true).unwrap();
                    assert!(!child.contains("Flagship enables delegation"), "{scenario}");
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
                    assert!(
                        !selected.contains("Flagship enables delegation"),
                        "{scenario}"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
