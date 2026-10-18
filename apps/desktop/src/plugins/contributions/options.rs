//! Package-scoped public options use the existing session configuration owner.
use super::*;
use sailry_protocol::plugin::settings::{Kind, Schema};
use serde_json::Value;

impl Metadata {
    pub(crate) fn operation_available(
        &self,
        operation: sailry_protocol::tool::Operation,
        project: Option<sailry_protocol::ProjectId>,
        resource: Option<sailry_protocol::connection::Resource>,
        assistant: Option<&sailry_protocol::plugin::conversation::Binding>,
    ) -> bool {
        use sailry_protocol::plugin::conversation::Tool;
        let metadata = self;
        let selected = assistant.and_then(|binding| {
            metadata
                .entries
                .values()
                .find(|info| info.summary.reference() == binding.package)?
                .extension
                .as_ref()?
                .desktop
                .as_ref()?
                .conversations
                .iter()
                .find(|declaration| declaration.id == binding.id)
        });
        if assistant.is_some() && selected.is_none() {
            return false;
        }
        metadata.entries.values().any(|info| {
            let Some(extension) = &info.extension else {
                return false;
            };
            info.summary.enabled
                && operation
                    .action()
                    .is_none_or(|action| extension.actions.contains(&action))
                && extension.tools.iter().any(|tool| {
                    tool.operation
                        .or_else(|| tool.handler.as_ref().and_then(|handler| handler.operation))
                        == Some(operation)
                        && tool.valid()
                        && tool.platforms.is_empty()
                        && tool.available_in(project, resource)
                        && !(resource.is_some() && assistant.is_none() && tool.contexts.is_empty())
                        && selected.is_none_or(|selected| {
                            selected.tools.iter().any(|entry| match entry {
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
                })
        })
    }

    fn option_schema(&self, owner: &Reference) -> Option<Schema> {
        let info = self.entries.get(&owner.name)?;
        if !info.summary.enabled || info.summary.reference() != *owner {
            return None;
        }
        serde_json::from_value(info.extension.as_ref()?.conversation_options.clone()?).ok()
    }

    pub(crate) fn options(
        &self,
        owner: &Reference,
        saved: Option<&BTreeMap<String, Value>>,
    ) -> BTreeMap<String, Value> {
        let Some(schema) = self.option_schema(owner) else {
            return Default::default();
        };
        project(schema, saved)
    }

    pub(crate) fn option_declared(&self, owner: &Reference, key: &str, value: &Value) -> bool {
        self.option_schema(owner)
            .and_then(|schema| schema.properties.get(key).cloned())
            .is_some_and(|field| match field.kind {
                Kind::Boolean => value.is_boolean(),
                Kind::String => value.is_string(),
                Kind::Integer => value.is_i64() || value.is_u64(),
                Kind::Number => value.is_number(),
            })
    }

    pub(crate) fn operations(
        &self,
        project: Option<sailry_protocol::ProjectId>,
        resource: Option<sailry_protocol::connection::Resource>,
        assistant: Option<&sailry_protocol::plugin::conversation::Binding>,
    ) -> Vec<sailry_protocol::tool::Operation> {
        let mut operations = Vec::new();
        for info in self.entries.values() {
            for tool in info.extension.iter().flat_map(|extension| &extension.tools) {
                if let Some(operation) = tool
                    .operation
                    .or_else(|| tool.handler.as_ref().and_then(|handler| handler.operation))
                    && !operations.contains(&operation)
                    && self.operation_available(operation, project, resource, assistant)
                {
                    operations.push(operation);
                }
            }
        }
        operations
    }
}

fn project(schema: Schema, saved: Option<&BTreeMap<String, Value>>) -> BTreeMap<String, Value> {
    schema
        .properties
        .into_iter()
        .filter_map(|(key, field)| {
            saved
                .and_then(|saved| saved.get(&key))
                .cloned()
                .or(field.default)
                .map(|value| (key, value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use serde_json::json;

    #[test]
    fn projects_declared_values_and_defaults() {
        let schema = || {
            serde_json::from_value(json!({
            "$schema":sailry_protocol::plugin::settings::SCHEMA,"type":"object",
            "properties":{"choice":{"type":"boolean","default":false},"level":{"type":"integer","default":2}},
            "required":["choice"],"additionalProperties":false
        })).unwrap()
        };
        assert_eq!(
            project(schema(), None),
            BTreeMap::from([("choice".into(), json!(false)), ("level".into(), json!(2))])
        );
        let saved = BTreeMap::from([
            ("choice".into(), json!(true)),
            ("removed".into(), json!("old")),
        ]);
        assert_eq!(
            project(schema(), Some(&saved)),
            BTreeMap::from([("choice".into(), json!(true)), ("level".into(), json!(2))])
        );
    }
}
