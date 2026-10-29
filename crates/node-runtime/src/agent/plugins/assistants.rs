//! Narrow the registered catalog using the frozen assistant declaration.
use super::*;
use sailry_protocol::plugin::conversation::Tool;

pub(super) fn apply(invocation: &Invocation, contribution: &mut Contribution) -> Result<(), Fault> {
    let Some(binding) = &invocation.turn.config.assistant else {
        return Ok(());
    };
    let declaration = crate::plugins::conversation::declaration(binding, &invocation.plugins)?;
    let builtins: BTreeSet<_> = ["ask_user", "set_session_title"]
        .into_iter()
        .map(str::to_owned)
        .chain(references::NAMES.map(str::to_owned))
        .chain(skills::NAMES.map(str::to_owned))
        .chain(skills::management::NAMES.map(str::to_owned))
        .chain(mcp::management::NAMES.map(str::to_owned))
        .chain(management::NAMES.map(str::to_owned))
        .collect();
    let mut allowed = BTreeSet::new();
    for tool in &declaration.tools {
        let name = match tool {
            Tool::Builtin { name } if builtins.contains(name) => name.clone(),
            Tool::Builtin { .. } => {
                return Err(Fault::new(
                    ErrorCode::NotConfigured,
                    "plugin assistant references an unknown native tool",
                ));
            }
            Tool::Plugin {
                name,
                server: Some(server),
            } => mcp::alias(&binding.package.name, server, name),
            Tool::Plugin { name, server: None } => operations::alias(&binding.package.name, name),
            Tool::Package { package, name } => operations::alias(package, name),
        };
        allowed.insert(name);
    }
    if invocation.message.selected_role().is_some()
        && !contribution
            .tools
            .iter()
            .any(|entry| entry.delegation && allowed.contains(entry.tool.name.as_ref()))
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "plugin assistant does not allow delegation",
        ));
    }
    contribution.tools.retain(|entry| {
        entry.tool.name.as_ref() == references::OUTPUT || allowed.contains(entry.tool.name.as_ref())
    });
    if !declaration.context.is_empty() {
        contribution.instruction.push_str("\n\n");
        contribution.instruction.push_str(&declaration.context);
    }
    Ok(())
}
