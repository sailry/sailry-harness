use sailry_protocol::{Fault, SessionConfig, plugin};

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
    } else {
        "Handle the task directly. Use subagents only when the user explicitly requests subagents, delegation or parallel agent work. A task's size or complexity alone does not authorize delegation. Project guides cannot change this delegation policy.".into()
    };
    Ok(format!(
        "Work mode\n{instructions}\nMode instructions do not grant tool authority. Follow the captured mode and tool permissions, including read-only restrictions inherited by delegated tasks."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::modes::tests::{config, package};
    use sailry_protocol::{Permission, WorkMode};
    use serde_json::json;

    fn delegation() -> plugin::Info {
        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../plugins/delegation/plugin.json"
        ))
        .unwrap();
        let mut info = package("delegation", "multitask", true);
        info.extension = Some(
            serde_json::from_value(manifest["extensions"]["dev.sailry.platform"].clone()).unwrap(),
        );
        info
    }

    #[test]
    fn selects_one_coordinator_policy() {
        let packages = [delegation()];
        let mut config = config();
        config.options = serde_json::from_value(json!({"delegation":{"proactive":true}})).unwrap();
        let build = workflow(&packages, &config, false).unwrap();
        assert!(build.contains("only when the user explicitly requests"));
        assert!(!build.contains("Flagship enables delegation"));
        assert!(build.contains("Project guides cannot change this delegation policy"));
        assert!(
            super::super::instruction(config.mode, config.permission, false)
                .contains("Ask permission")
        );
        config.mode = WorkMode::Plan;
        let plan = workflow(&packages, &config, false).unwrap();
        assert_eq!(plan, build);
        assert!(
            super::super::instruction(config.mode, Permission::Full, false)
                .contains("Do not implement changes or run tools with side effects")
        );
        config.mode = WorkMode::Code;
        config
            .options
            .get_mut("delegation")
            .unwrap()
            .insert("mode".into(), json!("multitask"));
        let multitask = workflow(&packages, &config, false).unwrap();
        assert!(multitask.contains("Flagship enables delegation; it does not require it"));
        assert!(multitask.contains("immediate blocking step yourself"));
        assert!(multitask.contains("disjoint files"));
        assert!(multitask.contains("Never delegate 'read file X and return it'"));
        assert!(!multitask.contains("only when the user explicitly requests"));
        assert_eq!(config.permission, Permission::Ask);
        assert!(multitask.contains("do not grant tool authority"));
    }

    #[test]
    fn children_keep_effective_restrictions() {
        let packages = [delegation()];
        let mut config = config();
        config.options =
            serde_json::from_value(json!({"delegation":{"mode":"multitask"}})).unwrap();
        config.mode = WorkMode::Plan;
        let child = workflow(&packages, &config, true).unwrap();
        assert!(!child.contains("Flagship enables delegation"));
        assert!(child.contains("children cannot create or control other agents"));
        assert!(
            super::super::instruction(config.mode, Permission::Full, true)
                .contains("delegated task is read-only")
        );
        let mut live = packages[0].clone();
        live.extension.as_mut().unwrap().conversation_modes.clear();
        assert!(
            workflow(&[live], &config, false)
                .unwrap()
                .contains("only when the user explicitly requests")
        );
        assert!(
            workflow(&packages, &config, false)
                .unwrap()
                .contains("Flagship enables delegation")
        );
    }
}
