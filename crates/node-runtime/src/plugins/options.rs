//! Public package choices share the settings schema validator and turn capture.
use sailry_protocol::{Fault, SessionConfig, plugin};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) fn schema(
    extension: &plugin::Extension,
) -> Result<Option<plugin::settings::Schema>, Fault> {
    extension
        .conversation_options
        .as_ref()
        .map(super::settings::option_schema)
        .transpose()
}

pub(crate) fn project(
    extension: &plugin::Extension,
    saved: Option<&BTreeMap<String, Value>>,
) -> Result<BTreeMap<String, Value>, Fault> {
    let Some(schema) = schema(extension)? else {
        if saved.is_some_and(|values| !values.is_empty()) {
            return Err(Fault::invalid(
                "package did not declare conversation options",
            ));
        }
        return Ok(Default::default());
    };
    let mut values: BTreeMap<_, _> = schema
        .properties
        .iter()
        .filter_map(|(key, field)| field.default.clone().map(|value| (key.clone(), value)))
        .collect();
    values.extend(
        saved
            .into_iter()
            .flatten()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    super::settings::validate_options(&schema, &values)?;
    Ok(values)
}

pub(crate) fn captured(
    info: &plugin::Info,
    config: &SessionConfig,
) -> Result<BTreeMap<String, Value>, Fault> {
    match &info.extension {
        Some(extension) => {
            let Some(schema) = schema(extension)? else {
                return Ok(Default::default());
            };
            let saved = config.options.get(&info.summary.name).map(|values| {
                values
                    .iter()
                    .filter(|(key, _)| schema.properties.contains_key(*key))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect()
            });
            project(extension, saved.as_ref())
        }
        None => Ok(Default::default()),
    }
}

#[cfg(test)]
mod tests;
