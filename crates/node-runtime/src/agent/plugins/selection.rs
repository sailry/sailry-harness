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
        if let Some(selected) = selected {
            return selected
                .iter()
                .any(|selection| matches(invocation, selection, entry));
        }
        if entry.plugin.is_none() {
            return references::NAMES.contains(&entry.tool.name())
                || skills::NAMES.contains(&entry.tool.name());
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
        if extension
            .model_tools
            .iter()
            .any(|tool| model_tools::names(tool.capability).contains(&entry.tool.name()))
        {
            return true;
        }
        extension.tools.iter().any(|tool| {
            operations::alias(&package.summary.name, &tool.name) == entry.tool.name()
                && standard(tool)
        })
    });
    Ok(())
}

fn matches(invocation: &Invocation, selection: &Selection, entry: &ToolRegistration) -> bool {
    match selection {
        Selection::Builtin { name } => entry.plugin.is_none() && entry.tool.name() == name,
        Selection::Package { package, name } => {
            entry.plugin.as_ref() == Some(package)
                && invocation.plugins.iter().any(|provider| {
                    provider.summary.name == *package
                        && provider.extension.as_ref().is_some_and(|extension| {
                            extension.tools.iter().any(|tool| {
                                tool.name == *name
                                    && entry.tool.name()
                                        == tool.server.as_ref().map_or_else(
                                            || operations::alias(package, name),
                                            |server| mcp::alias(package, server, name),
                                        )
                            })
                        })
                })
        }
        Selection::Model { capability } => {
            model_tools::names(*capability).contains(&entry.tool.name())
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

fn standard(tool: &sailry_protocol::tool::Declaration) -> bool {
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
    !operations.is_empty()
        && operations.into_iter().all(|operation| {
            matches!(
                operation,
                Operation::ReadFile
                    | Operation::ReadFileLines
                    | Operation::ListDirectory
                    | Operation::FindFiles
                    | Operation::SearchFiles
                    | Operation::WriteFile
                    | Operation::EditFile
                    | Operation::DeleteFile
                    | Operation::MoveFile
                    | Operation::CreateDirectory
                    | Operation::ReadOffice
                    | Operation::ReadNotebook
                    | Operation::EditNotebook
                    | Operation::SearchCode
                    | Operation::OutlineCode
                    | Operation::InspectGit
                    | Operation::ReadGitDiff
                    | Operation::ReadGitLog
                    | Operation::ReadGitShow
                    | Operation::ReadGitBlame
                    | Operation::ListGitChanges
                    | Operation::ReadGitChange
                    | Operation::ListWorktrees
                    | Operation::RunCommand
                    | Operation::ReadCommand
                    | Operation::StopCommand
                    | Operation::FetchWeb
                    | Operation::SearchWeb
                    | Operation::Progress
                    | Operation::ListRoles
            )
        })
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
        ] {
            assert!(standard(&tool(operation)));
        }
        for operation in [
            Operation::ReadBrowser,
            Operation::ControlBrowser,
            Operation::QueryDatabase,
            Operation::RunSsh,
            Operation::GenerateImage,
            Operation::ScheduleJob,
        ] {
            assert!(!standard(&tool(operation)));
        }
    }
}
