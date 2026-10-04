//! Narrow the registered catalog using the frozen assistant declaration.
use super::*;
use sailry_protocol::plugin::conversation::Tool;

pub(super) fn apply(invocation: &Invocation, contribution: &mut Contribution) -> Result<(), Fault> {
    let Some(binding) = &invocation.turn.config.assistant else {
        return Ok(());
    };
    let declaration = crate::plugins::conversation::declaration(binding, &invocation.plugins)?;
    let builtins: BTreeSet<_> = [
        "ask_user",
        "set_session_title",
        "compact_context",
        "read_session",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain(skills::NAMES.map(str::to_owned))
    .chain(skills::management::NAMES.map(str::to_owned))
    .collect();
    let mut allowed = BTreeSet::new();
    for tool in &declaration.tools {
        let name = match tool {
            Tool::Model { capability } => {
                allowed.extend(
                    model_tools::names(*capability)
                        .iter()
                        .map(|name| name.to_string()),
                );
                continue;
            }
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
            Tool::Plugin { name, server: None } => {
                identity(invocation, &binding.package.name, name)
            }
            Tool::Package { package, name } => identity(invocation, package, name),
        };
        allowed.insert(name);
    }
    if invocation.message.selected_role().is_some()
        && !contribution
            .tools
            .iter()
            .any(|entry| entry.tool.is_agent_delegation() && allowed.contains(entry.tool.name()))
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "plugin assistant does not allow delegation",
        ));
    }
    contribution
        .tools
        .retain(|entry| allowed.contains(entry.tool.name()));
    if !declaration.context.is_empty() {
        contribution.instruction.push_str("\n\n");
        contribution.instruction.push_str(&declaration.context);
    }
    Ok(())
}

fn identity(invocation: &Invocation, package: &str, name: &str) -> String {
    if invocation.plugins.iter().any(|provider| {
        provider.summary.name == package
            && crate::plugins::tools::computer(provider, name).is_some()
    }) {
        name.into()
    } else {
        operations::alias(package, name)
    }
}
