//! A role selects declarations; mounting never grants execution authority.
use super::*;
use sailry_protocol::{plugin::conversation::Tool as Selection, tool::Operation};

pub(super) fn apply(invocation: &Invocation, contribution: &mut Contribution) -> Result<(), Fault> {
    let Some(child) = &invocation.child else {
        return Ok(());
    };
    let selected = child.role.as_ref().and_then(|role| role.tools.as_deref());
    if let Some(selected) = selected
        && selected.iter().any(|selection| {
            !contribution
                .tools
                .iter()
                .any(|entry| matches(invocation, selection, entry))
        })
    {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "role references an unavailable tool",
        ));
    }
    contribution.tools.retain(|entry| {
        // Paging an already-visible result adds no execution capability.
        if entry.tool.name.as_ref() == references::OUTPUT {
            return true;
        }
        if let Some(selected) = selected {
            return selected
                .iter()
                .any(|selection| matches(invocation, selection, entry));
        }
        if entry.plugin.is_none() {
            return references::NAMES.contains(&entry.tool.name.as_ref())
                || skills::NAMES.contains(&entry.tool.name.as_ref());
        }
        let Some(package) = invocation
            .plugins
            .iter()
            .find(|package| entry.plugin.as_deref() == Some(package.summary.name.as_str()))
        else {
            return false;
        };
        let Some(extension) = &package.extension else {
            return false;
        };
        extension.tools.iter().any(|tool| {
            operations::alias(&package.summary.name, &tool.name) == entry.tool.name.as_ref()
                && standard(tool)
        })
    });
    Ok(())
}

/// Common worktree capabilities stay visible; other captured tools are discoverable.
pub(super) fn defer(invocation: &Invocation, tools: &mut [ToolRegistration]) -> Result<(), Fault> {
    if invocation.turn.config.assistant.is_some()
        || invocation
            .child
            .as_ref()
            .and_then(|child| child.role.as_ref())
            .is_some_and(|role| role.tools.is_some())
    {
        return Ok(());
    }
    let selected = crate::plugins::modes::selected(&invocation.plugins, &invocation.turn.config)?
        .map(|(package, _)| package.summary.name.as_str());
    for entry in tools {
        entry.deferred = !initial(invocation, selected, entry);
    }
    Ok(())
}

fn initial(invocation: &Invocation, selected: Option<&str>, entry: &ToolRegistration) -> bool {
    if entry.delegation && invocation.message.selected_role().is_some() {
        return true;
    }
    let Some(owner) = entry.plugin.as_deref() else {
        return references::NAMES.contains(&entry.tool.name.as_ref())
            || skills::NAMES.contains(&entry.tool.name.as_ref());
    };
    if selected == Some(owner) {
        return true;
    }
    invocation
        .plugins
        .iter()
        .find(|package| package.summary.name == owner)
        .and_then(|package| package.extension.as_ref())
        .is_some_and(|extension| {
            extension.tools.iter().any(|tool| {
                operations::alias(owner, &tool.name) == entry.tool.name.as_ref() && visible(tool)
            })
        })
}

fn matches(invocation: &Invocation, selection: &Selection, entry: &ToolRegistration) -> bool {
    match selection {
        Selection::Builtin { name } => entry.plugin.is_none() && entry.tool.name.as_ref() == name,
        Selection::Package { package, name } => {
            entry.plugin.as_ref() == Some(package)
                && invocation.plugins.iter().any(|provider| {
                    provider.summary.name == *package
                        && provider.extension.as_ref().is_some_and(|extension| {
                            extension.tools.iter().any(|tool| {
                                tool.name == *name
                                    && entry.tool.name.as_ref()
                                        == tool.server.as_ref().map_or_else(
                                            || operations::alias(package, name),
                                            |server| mcp::alias(package, server, name),
                                        )
                            })
                        })
                })
        }
        Selection::Plugin { .. } => false,
    }
}

pub(super) fn declared(
    invocation: &Invocation,
    package: &sailry_protocol::plugin::Info,
    tool: &sailry_protocol::tool::Declaration,
) -> bool {
    let Some(child) = &invocation.child else {
        return true;
    };
    if let Some(selected) = child.role.as_ref().and_then(|role| role.tools.as_deref()) {
        selected.iter().any(|selection| {
            matches!(selection, Selection::Package { package: owner, name }
            if *owner == package.summary.name && *name == tool.name)
        })
    } else {
        standard(tool)
    }
}

/// The delegated catalog retains its standard execution authority.
fn standard(tool: &sailry_protocol::tool::Declaration) -> bool {
    declares(tool, |operation| {
        visible_operation(operation)
            || matches!(
                operation,
                Operation::DeleteFile
                    | Operation::MoveFile
                    | Operation::CreateDirectory
                    | Operation::ReadOffice
                    | Operation::ReadNotebook
                    | Operation::ReadArtifact
                    | Operation::ListArtifacts
                    | Operation::EditNotebook
                    | Operation::ReadGitLog
                    | Operation::ReadGitShow
                    | Operation::ReadGitBlame
                    | Operation::ListGitChanges
                    | Operation::ReadGitChange
                    | Operation::ListWorktrees
            )
    })
}

/// The main agent starts with everyday capabilities; the rest stay discoverable.
fn visible(tool: &sailry_protocol::tool::Declaration) -> bool {
    declares(tool, visible_operation)
}

fn visible_operation(operation: Operation) -> bool {
    matches!(
        operation,
        Operation::ReadFile
            | Operation::ReadFileLines
            | Operation::ListDirectory
            | Operation::FindFiles
            | Operation::SearchFiles
            | Operation::WriteFile
            | Operation::EditFile
            | Operation::SearchCode
            | Operation::OutlineCode
            | Operation::InspectGit
            | Operation::ReadGitDiff
            | Operation::RunCommand
            | Operation::ReadCommand
            | Operation::StopCommand
            | Operation::FetchWeb
            | Operation::SearchWeb
            | Operation::Progress
            | Operation::ListRoles
            | Operation::ListArtifactRevisions
    )
}

fn declares(
    tool: &sailry_protocol::tool::Declaration,
    accepted: impl Fn(Operation) -> bool,
) -> bool {
    let operations: Vec<_> = tool
        .operation
        .into_iter()
        .chain(tool.handler.iter().flat_map(|handler| {
            handler.operation.into_iter().chain(
                handler
                    .flow
                    .iter()
                    .flat_map(|flow| flow.operations.iter().copied()),
            )
        }))
        .collect();
    !operations.is_empty() && operations.into_iter().all(accepted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_declared_operations() {
        let tool = |operation| sailry_protocol::tool::Declaration {
            name: "fixture".into(),
            server: None,
            presentation: Default::default(),
            grouping: Default::default(),
            operation: Some(operation),
            handler: None,
            description: None,
            display: None,
            platforms: vec![],
            contexts: vec![],
            platform_descriptions: Default::default(),
        };
        for operation in [
            Operation::ReadFile,
            Operation::RunCommand,
            Operation::ReadCommand,
            Operation::InspectGit,
            Operation::ListRoles,
            Operation::ReadArtifact,
            Operation::ListArtifacts,
            Operation::ListArtifactRevisions,
        ] {
            assert!(standard(&tool(operation)));
        }
        for operation in [
            Operation::ReadBrowser,
            Operation::ControlBrowser,
            Operation::QueryDatabase,
            Operation::RunSsh,
            Operation::GenerateImage,
            Operation::StorageTransaction,
            Operation::WriteArtifact,
        ] {
            assert!(!standard(&tool(operation)));
        }
        for operation in [
            Operation::ReadFile,
            Operation::EditFile,
            Operation::SearchFiles,
            Operation::InspectGit,
            Operation::ReadGitDiff,
            Operation::RunCommand,
            Operation::Progress,
            Operation::ListArtifactRevisions,
        ] {
            assert!(visible(&tool(operation)));
        }
        for operation in [
            Operation::DeleteFile,
            Operation::MoveFile,
            Operation::CreateDirectory,
            Operation::ReadOffice,
            Operation::ReadNotebook,
            Operation::EditNotebook,
            Operation::ReadGitLog,
            Operation::ReadGitShow,
            Operation::ReadGitBlame,
            Operation::ListGitChanges,
            Operation::ReadGitChange,
            Operation::ReadArtifact,
            Operation::ListArtifacts,
            Operation::ListWorktrees,
        ] {
            assert!(standard(&tool(operation)));
            assert!(!visible(&tool(operation)));
        }
    }
}
