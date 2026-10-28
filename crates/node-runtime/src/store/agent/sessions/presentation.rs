//! Resolve captured tool labels and command aliases outside the storage worker.
use super::*;
use sailry_protocol::{
    connection::Resource,
    tool::{Alias, AliasKind, InputFormat, Operation},
};

impl Sessions {
    pub(super) async fn current_presentation(
        &self,
        calls: Vec<(String, Value)>,
    ) -> Result<Presentation, Fault> {
        let mut captured = self.presentation.clone();
        let mut pending = Vec::new();
        for (name, arguments) in &calls {
            let Some(field) = captured
                .displays
                .get(name)
                .and_then(|display| display.input.as_ref())
                .and_then(|input| input.summary.as_ref())
                .filter(|field| field.format == Some(InputFormat::Connection))
            else {
                continue;
            };
            let Some(id) = field.read(arguments) else {
                continue;
            };
            if field.names.contains_key(id) {
                continue;
            }
            let Some(binding) = self.bindings.get(name) else {
                continue;
            };
            let resource = match binding.operation {
                Some(Operation::RunSsh | Operation::TransferSsh) => {
                    id.parse().ok().map(Resource::Ssh)
                }
                Some(
                    Operation::BrowseDatabase
                    | Operation::QueryDatabase
                    | Operation::ExecuteDatabase,
                ) => id.parse().ok().map(Resource::Database),
                _ => None,
            };
            if let Some(resource) = resource {
                pending.push((name.clone(), id.to_owned(), binding.owner.clone(), resource));
            }
        }
        if !pending.is_empty() {
            let names = self
                .ingress
                .connection_names(
                    self.turn,
                    pending
                        .iter()
                        .map(|(_, _, owner, target)| (owner.clone(), *target))
                        .collect(),
                    self.connections.clone(),
                )
                .await?;
            for ((tool, id, _, _), name) in pending.into_iter().zip(names) {
                if let Some(name) = name
                    && let Some(field) = captured
                        .displays
                        .get_mut(&tool)
                        .and_then(|display| display.input.as_mut())
                        .and_then(|input| input.summary.as_mut())
                {
                    field.names.insert(id, name);
                }
            }
        }
        let mut aliases = Vec::new();
        if let Some(home) = self.ingress.environment.home() {
            aliases.push(Alias {
                path: home.to_string_lossy().into_owned(),
                kind: AliasKind::Home,
            });
        }
        let commands: Vec<_> = calls
            .iter()
            .filter_map(|(name, args)| {
                let field = captured
                    .displays
                    .get(name)?
                    .input
                    .as_ref()?
                    .summary
                    .as_ref()?;
                (field.format == Some(InputFormat::Command))
                    .then(|| field.read(args))
                    .flatten()
            })
            .collect();
        if let Some(python) = self.ingress.plugins.display_python(&commands).await {
            aliases.push(Alias {
                path: python.to_string_lossy().into_owned(),
                kind: AliasKind::Python,
            });
        }
        for access in self.ingress.agents.python(self.turn) {
            let alias = Alias {
                path: access.ready.python.to_string_lossy().into_owned(),
                kind: AliasKind::Python,
            };
            if aliases.len() < 32 && !aliases.contains(&alias) {
                aliases.push(alias);
            }
        }
        for display in captured.displays.values_mut() {
            display.aliases.clear();
            if display
                .input
                .as_ref()
                .and_then(|input| input.summary.as_ref())
                .is_some_and(|field| field.format == Some(InputFormat::Command))
            {
                display.aliases.clone_from(&aliases);
            }
        }
        Ok(captured)
    }
}
