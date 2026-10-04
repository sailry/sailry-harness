use super::*;
use sailry_protocol::conversation::catalog;
use serde_json::Value;
use std::collections::BTreeSet;

pub(crate) struct Entry {
    pub provider: String,
    pub id: String,
    /// Retain source metadata for reference pricing without executing provider overrides.
    pub body: Vec<u8>,
}

pub(super) fn parse(bytes: &[u8]) -> Result<(u32, Vec<Entry>), Fault> {
    let catalog: Value =
        serde_json::from_slice(bytes).map_err(|_| unavailable("model catalog JSON is invalid"))?;
    let providers = catalog
        .as_object()
        .filter(|providers| !providers.is_empty() && providers.len() <= 1000)
        .ok_or_else(|| unavailable("model catalog providers are invalid"))?;
    let mut entries = Vec::new();
    let mut ids = BTreeSet::new();
    for (provider, value) in providers {
        identifier(provider, 128)?;
        let models = value["models"]
            .as_object()
            .ok_or_else(|| unavailable("model catalog models are invalid"))?;
        for value in models.values() {
            let model = model(value)?;
            if !ids.insert((provider.clone(), model.id.clone())) {
                return Err(unavailable("model catalog has duplicate model identifiers"));
            }
            let body = serde_json::to_vec(value)
                .map_err(|_| unavailable("model catalog serialization failed"))?;
            if body.len() > 128 * 1024 || entries.len() >= 100_000 {
                return Err(unavailable("model catalog entries exceed their limits"));
            }
            entries.push(Entry {
                provider: provider.clone(),
                id: model.id,
                body,
            });
        }
    }
    Ok((providers.len() as u32, entries))
}

pub(crate) fn model(value: &Value) -> Result<catalog::Model, Fault> {
    let id = value["id"]
        .as_str()
        .ok_or_else(|| unavailable("catalog model identifier is missing"))?;
    identifier(id, 256)?;
    let name = match value.get("name") {
        None | Some(Value::Null) => id,
        Some(value) => value
            .as_str()
            .ok_or_else(|| unavailable("catalog model name is invalid"))?,
    };
    let name = name.trim();
    identifier(name, 512)?;
    for key in ["limit", "modalities"] {
        if value
            .get(key)
            .is_some_and(|value| !value.is_null() && !value.is_object())
        {
            return Err(unavailable("catalog model metadata is invalid"));
        }
    }
    let options: Vec<catalog::Reasoning> = match value.get("reasoning_options") {
        None | Some(Value::Null) => vec![],
        Some(value) => serde_json::from_value(value.clone())
            .map_err(|_| unavailable("catalog reasoning options are invalid"))?,
    };
    if options.len() > 16 {
        return Err(unavailable("catalog reasoning options exceed their limit"));
    }
    for option in &options {
        match option {
            catalog::Reasoning::Effort { values } => {
                if values.len() > 32 {
                    return Err(unavailable("catalog reasoning values exceed their limit"));
                }
                for value in values.iter().flatten() {
                    identifier(value, 64)?;
                }
            }
            catalog::Reasoning::BudgetTokens { min, max }
                if min.is_some_and(|min| min < -1)
                    || min
                        .zip(*max)
                        .is_some_and(|(min, max)| min >= 0 && min as u32 > max) =>
            {
                return Err(unavailable("catalog reasoning budget is invalid"));
            }
            _ => {}
        }
    }
    Ok(catalog::Model {
        id: id.to_owned(),
        name: name.to_owned(),
        context: limit(&value["limit"], "context")?,
        output: limit(&value["limit"], "output")?,
        inputs: modalities(value, "input")?,
        outputs: modalities(value, "output")?,
        tools: optional_bool(value, "tool_call")?,
        reasoning: optional_bool(value, "reasoning")?,
        options,
    })
}

pub(crate) fn identifier(value: &str, limit: usize) -> Result<(), Fault> {
    if value.trim().is_empty() || value.len() > limit || value.chars().any(char::is_control) {
        return Err(invalid("catalog identifier is invalid"));
    }
    Ok(())
}

fn limit(value: &Value, key: &str) -> Result<Option<u32>, Fault> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .map(|value| (value > 0).then_some(value))
            .ok_or_else(|| unavailable("catalog token limit is invalid")),
    }
}

fn optional_bool(value: &Value, key: &str) -> Result<Option<bool>, Fault> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_bool()
            .map(Some)
            .ok_or_else(|| unavailable("catalog capability is invalid")),
    }
}

fn modalities(value: &Value, direction: &str) -> Result<Vec<String>, Fault> {
    let value = &value["modalities"][direction];
    if value.is_null() {
        return Ok(vec![]);
    }
    let values = value
        .as_array()
        .filter(|values| values.len() <= 16)
        .ok_or_else(|| unavailable("catalog modalities are invalid"))?;
    values
        .iter()
        .map(|value| {
            let value = value
                .as_str()
                .ok_or_else(|| unavailable("catalog modality is invalid"))?;
            identifier(value, 32)?;
            Ok(value.to_owned())
        })
        .collect()
}
