//! Explicit mentions select a capability for this task without widening tool permissions.
use super::*;
use sailry_protocol::conversation::reference::{Reference, Target};
use std::collections::{BTreeMap, BTreeSet};

pub(in crate::agent) fn tools(tools: &[catalog::Registration]) -> BTreeMap<String, Vec<String>> {
    let mut captured = BTreeMap::<String, Vec<String>>::new();
    for tool in tools {
        if let Some(plugin) = &tool.plugin {
            captured
                .entry(plugin.clone())
                .or_default()
                .push(tool.tool.name().to_owned());
        }
    }
    captured
}

pub(in crate::agent) async fn instruction<'a>(
    invocation: &Invocation,
    references: impl Iterator<Item = &'a Reference>,
    resources: &crate::plugins::resources::Resources,
    tools: &BTreeMap<String, Vec<String>>,
) -> Result<String, Fault> {
    let mut result = String::new();
    let mut loaded = BTreeSet::new();
    for reference in references {
        match &reference.target {
            Target::Plugin(name) => {
                result.push_str(&format!("\n\nThe user explicitly selected plugin {name} for this task. Use its applicable capabilities to fulfill the request. This selection does not itself execute an action or grant additional permissions."));
                if let Some(names) = tools.get(name) {
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
                        result.push_str("\nIf load_skill is available in this turn's tool catalog, load the relevant skill before following its workflow unless its full instructions are already supplied. Otherwise use already supplied full instructions or explain that skill loading is unavailable; do not follow a partly loaded workflow or load unrelated skills.");
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
            return Err(sailry_protocol::Fault::invalid(
                "selected capability instructions exceed the context budget",
            ));
        }
    }
    Ok(result)
}
