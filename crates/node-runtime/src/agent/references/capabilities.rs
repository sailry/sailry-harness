//! Explicit mentions select a capability for this task without widening tool permissions.
use super::*;
use std::collections::BTreeSet;

pub(in crate::agent) async fn instruction(
    invocation: &Invocation,
    resources: &crate::plugins::resources::Resources,
    tools: &[catalog::Registration],
) -> Result<String, Fault> {
    let mut result = String::new();
    let mut loaded = BTreeSet::new();
    for reference in &invocation.message.references {
        match &reference.target {
            Target::Plugin(name) => {
                let names: Vec<_> = tools
                    .iter()
                    .filter(|tool| tool.plugin.as_deref() == Some(name.as_str()))
                    .map(|tool| tool.tool.name())
                    .collect();
                result.push_str(&format!("\n\nThe user explicitly selected plugin {name} for this task. Use its applicable capabilities to fulfill the request. This selection does not itself execute an action or grant additional permissions."));
                if !names.is_empty() {
                    result.push_str(&format!("\nAvailable tools: {}", names.join(", ")));
                }
                if let Some(package) = invocation
                    .plugins
                    .iter()
                    .find(|package| package.summary.name == *name)
                {
                    for skill in &package.skills {
                        result.push_str(&format!(
                            "\nSkill {name}:{} — {}",
                            skill.name, skill.description
                        ));
                    }
                    if !package.skills.is_empty() {
                        result.push_str("\nLoad the relevant skill with load_skill before following its workflow; do not load unrelated skills.");
                    }
                }
                result.push_str("\nIf a required capability is unavailable, explain that limitation instead of claiming the plugin was used.");
            }
            Target::Skill { package, name } => {
                let key = format!("{package}:{name}");
                if loaded.insert(key.clone()) {
                    let text = resources.read(&key, "SKILL.md").await?;
                    result.push_str(&format!("\n\nThe user explicitly selected skill {key}. Apply the following workflow to this task. User instructions and existing tool permissions still take precedence.\nSkill directory: {}\n<selected_skill>\n{}\n</selected_skill>", text.directory, text.content));
                }
            }
            _ => {}
        }
        if result.len() > 256 * 1024 {
            return Err(invalid(
                "selected capability instructions exceed the context budget",
            ));
        }
    }
    Ok(result)
}
