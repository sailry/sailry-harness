//! Native operation projection follows the Metadata owner and session context.
use super::Metadata;
use sailry_protocol::{
    ProjectId,
    connection::Resource,
    plugin::conversation::{Binding, Tool},
    tool::Operation,
};

pub(super) struct Cached {
    project: Option<ProjectId>,
    resource: Option<Resource>,
    assistant: Option<Binding>,
    values: Vec<Operation>,
}

impl Metadata {
    pub(crate) fn operations(
        &self,
        project: Option<ProjectId>,
        resource: Option<Resource>,
        assistant: Option<&Binding>,
    ) -> Vec<Operation> {
        if let Some(cached) = self.operations.borrow().as_ref()
            && cached.project == project
            && cached.resource == resource
            && cached.assistant.as_ref() == assistant
        {
            return cached.values.clone();
        }
        let values = self.collect_operations(project, resource, assistant);
        // Inventory reconciliation and loaded metadata invalidate this projection.
        *self.operations.borrow_mut() = Some(Cached {
            project,
            resource,
            assistant: assistant.cloned(),
            values: values.clone(),
        });
        values
    }

    fn collect_operations(
        &self,
        project: Option<ProjectId>,
        resource: Option<Resource>,
        assistant: Option<&Binding>,
    ) -> Vec<Operation> {
        let selected = assistant.and_then(|binding| {
            self.entries
                .values()
                .find(|info| {
                    info.summary.available() && info.summary.reference() == binding.package
                })?
                .extension
                .as_ref()?
                .desktop
                .as_ref()?
                .conversations
                .iter()
                .find(|declaration| declaration.id == binding.id)
        });
        if assistant.is_some() && selected.is_none() {
            return Vec::new();
        }
        let mut operations: Vec<(Operation, bool)> = Vec::new();
        for info in self.entries.values() {
            let Some(extension) = &info.extension else {
                continue;
            };
            for tool in &extension.tools {
                let Some(operation) = tool
                    .operation
                    .or_else(|| tool.handler.as_ref().and_then(|handler| handler.operation))
                else {
                    continue;
                };
                let index = operations
                    .iter()
                    .position(|(value, _)| *value == operation)
                    .unwrap_or_else(|| {
                        operations.push((operation, false));
                        operations.len() - 1
                    });
                if operations[index].1
                    || !info.summary.available()
                    || operation
                        .action()
                        .is_some_and(|action| !extension.actions.contains(&action))
                    || !tool.platforms.is_empty()
                    || !tool.available_in(project, resource)
                    || (resource.is_some() && assistant.is_none() && tool.contexts.is_empty())
                    || selected.is_some_and(|selected| {
                        !selected.tools.iter().any(|entry| match entry {
                            Tool::Plugin { name, server: None } => {
                                assistant.is_some_and(|binding| {
                                    binding.package == info.summary.reference()
                                }) && *name == tool.name
                            }
                            Tool::Package { package, name } => {
                                *package == info.summary.name && *name == tool.name
                            }
                            _ => false,
                        })
                    })
                {
                    continue;
                }
                operations[index].1 = tool.valid();
            }
        }
        operations
            .into_iter()
            .filter_map(|(operation, available)| available.then_some(operation))
            .collect()
    }
}
