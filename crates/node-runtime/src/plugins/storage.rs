//! Package-owned logical schemas never mutate data while projecting defaults.
use sailry_protocol::{
    ErrorCode, Fault,
    plugin::storage::{self, Declaration},
};
use serde_json::Value;
use std::collections::BTreeMap;

struct NoRetrieval;

impl jsonschema::Retrieve for NoRetrieval {
    fn retrieve(
        &self,
        _: &jsonschema::Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        Err("storage schema retrieval is disabled".into())
    }
}

pub(crate) fn validate(declaration: &Declaration) -> Result<(), Fault> {
    if !declaration.valid() {
        return Err(invalid("invalid storage collection selectors"));
    }
    for collection in &declaration.collections {
        check_size(&collection.schema, storage::MAX_SCHEMA_BYTES)?;
        let mut defaults = Vec::new();
        inspect(
            &collection.schema,
            &collection.schema,
            "#",
            0,
            &mut defaults,
        )?;
        build(&collection.schema)?;
        if !defaults.is_empty() {
            let validators = jsonschema::draft202012::options()
                .with_retriever(NoRetrieval)
                .build_map(&collection.schema)
                .map_err(|_| invalid("invalid storage schema constraints"))?;
            for (pointer, value) in defaults {
                if !validators
                    .get(&pointer)
                    .is_some_and(|validator| validator.is_valid(value))
                {
                    return Err(invalid("invalid storage schema default"));
                }
            }
        }
    }
    Ok(())
}

fn build(schema: &Value) -> Result<jsonschema::Validator, Fault> {
    jsonschema::draft202012::options()
        .with_retriever(NoRetrieval)
        .build(schema)
        .map_err(|_| invalid("invalid storage schema constraints"))
}

fn inspect<'a>(
    root: &'a Value,
    schema: &'a Value,
    pointer: &str,
    depth: usize,
    defaults: &mut Vec<(String, &'a Value)>,
) -> Result<(), Fault> {
    if depth > storage::MAX_SCHEMA_DEPTH {
        return Err(invalid("storage schema exceeds the depth limit"));
    }
    let Some(object) = schema.as_object() else {
        return Ok(());
    };
    if object.get("additionalProperties") == Some(&Value::Bool(false))
        || object.get("unevaluatedProperties") == Some(&Value::Bool(false))
    {
        return Err(invalid(
            "closed storage objects cannot preserve unknown fields",
        ));
    }
    reference(root, schema)?;
    if object
        .get("$schema")
        .is_some_and(|value| value.as_str() != Some("https://json-schema.org/draft/2020-12/schema"))
        || object.contains_key("$dynamicRef")
        || (depth > 0 && object.contains_key("$id"))
        || object.get("$ref").is_some_and(|value| {
            !value
                .as_str()
                .is_some_and(|value| value == "#" || value.starts_with("#/"))
        })
    {
        return Err(invalid(
            "storage schemas require local JSON pointer references",
        ));
    }
    if let Some(value) = object.get("default") {
        defaults.push((pointer.into(), value));
    }
    for key in [
        "properties",
        "patternProperties",
        "$defs",
        "dependentSchemas",
    ] {
        if let Some(children) = object.get(key).and_then(Value::as_object) {
            for (name, child) in children {
                let escaped = name.replace('~', "~0").replace('/', "~1");
                inspect(
                    root,
                    child,
                    &format!("{pointer}/{key}/{escaped}"),
                    depth + 1,
                    defaults,
                )?;
            }
        }
    }
    for key in [
        "items",
        "additionalProperties",
        "unevaluatedProperties",
        "contains",
        "not",
        "if",
        "then",
        "else",
        "propertyNames",
        "unevaluatedItems",
        "contentSchema",
    ] {
        if let Some(child) = object.get(key) {
            inspect(
                root,
                child,
                &format!("{pointer}/{key}"),
                depth + 1,
                defaults,
            )?;
        }
    }
    for key in ["prefixItems", "allOf", "anyOf", "oneOf"] {
        if let Some(children) = object.get(key).and_then(Value::as_array) {
            for (index, child) in children.iter().enumerate() {
                inspect(
                    root,
                    child,
                    &format!("{pointer}/{key}/{index}"),
                    depth + 1,
                    defaults,
                )?;
            }
        }
    }
    Ok(())
}

pub(crate) fn project(schema: &Value, value: &mut Value) -> Result<(), Fault> {
    let bytes = check_size(value, storage::MAX_VALUE_BYTES)?;
    let mut budget = storage::MAX_VALUE_BYTES - bytes;
    defaults(schema, schema, value, 0, &mut budget, &mut Vec::new())?;
    check_size(value, storage::MAX_VALUE_BYTES)?;
    Ok(())
}

pub(crate) fn normalize(
    schema: &Value,
    previous: Option<&Value>,
    value: &Value,
) -> Result<Value, Fault> {
    check_size(value, storage::MAX_VALUE_BYTES)?;
    let mut value = value.clone();
    if let Some(previous) = previous {
        preserve(schema, &[schema], previous, &mut value, 0)?;
    }
    project(schema, &mut value)?;
    if !build(schema)?.is_valid(&value) {
        return Err(invalid("plugin value does not match its storage schema"));
    }
    Ok(value)
}

fn reference<'a>(root: &'a Value, schema: &'a Value) -> Result<Option<&'a Value>, Fault> {
    schema
        .get("$ref")
        .and_then(Value::as_str)
        .map(|reference| {
            root.pointer(reference.strip_prefix('#').unwrap_or(reference))
                .ok_or_else(|| invalid("storage schema reference is unavailable"))
        })
        .transpose()
}

fn defaults<'a>(
    root: &'a Value,
    schema: &'a Value,
    value: &mut Value,
    depth: usize,
    budget: &mut usize,
    visiting: &mut Vec<&'a Value>,
) -> Result<(), Fault> {
    if visiting.iter().any(|node| std::ptr::eq(*node, schema)) {
        return Ok(());
    }
    if depth > storage::MAX_SCHEMA_DEPTH {
        return Err(invalid(
            "storage default projection exceeds the depth limit",
        ));
    }
    visiting.push(schema);
    if let Some(target) = reference(root, schema)? {
        defaults(root, target, value, depth + 1, budget, visiting)?;
    }
    if let (Some(fields), Some(object)) = (
        schema.get("properties").and_then(Value::as_object),
        value.as_object_mut(),
    ) {
        for (name, field) in fields {
            if !object.contains_key(name)
                && let Some(default) = default(root, field)?
            {
                let bytes = serde_json::to_vec(default)
                    .map_err(|_| invalid("invalid storage default"))?
                    .len()
                    + serde_json::to_string(name)
                        .map_err(|_| invalid("invalid storage property"))?
                        .len()
                    + 2;
                *budget = budget
                    .checked_sub(bytes)
                    .ok_or_else(|| invalid("plugin value exceeds 256 KiB"))?;
                object.insert(name.clone(), default.clone());
            }
            if let Some(value) = object.get_mut(name) {
                defaults(root, field, value, depth + 1, budget, &mut Vec::new())?;
            }
        }
    }
    if let (Some(items), Some(values)) = (schema.get("items"), value.as_array_mut()) {
        let start = schema
            .get("prefixItems")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        for value in values.iter_mut().skip(start) {
            defaults(root, items, value, depth + 1, budget, &mut Vec::new())?;
        }
    }
    if let (Some(items), Some(values)) = (
        schema.get("prefixItems").and_then(Value::as_array),
        value.as_array_mut(),
    ) {
        for (item, value) in items.iter().zip(values) {
            defaults(root, item, value, depth + 1, budget, &mut Vec::new())?;
        }
    }
    visiting.pop();
    Ok(())
}

fn default<'a>(root: &'a Value, mut schema: &'a Value) -> Result<Option<&'a Value>, Fault> {
    let mut visiting = Vec::new();
    for _ in 0..=storage::MAX_SCHEMA_DEPTH {
        if visiting.iter().any(|node| std::ptr::eq(*node, schema)) {
            return Ok(None);
        }
        visiting.push(schema);
        if let Some(value) = schema.get("default") {
            return Ok(Some(value));
        }
        let Some(target) = reference(root, schema)? else {
            return Ok(None);
        };
        schema = target;
    }
    Err(invalid("storage default reference exceeds the depth limit"))
}

fn preserve(
    root: &Value,
    schemas: &[&Value],
    previous: &Value,
    value: &mut Value,
    depth: usize,
) -> Result<(), Fault> {
    if depth > storage::MAX_SCHEMA_DEPTH {
        return Err(invalid("storage normalization exceeds the depth limit"));
    }
    let (Some(old), Some(object)) = (previous.as_object(), value.as_object_mut()) else {
        return Ok(());
    };
    let mut nodes = Vec::new();
    for schema in schemas {
        object_schemas(root, schema, depth, &mut nodes)?;
    }
    let mut fields: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    let mut dictionaries = Vec::new();
    let mut patterned = false;
    let mut declared = false;
    for schema in nodes {
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            declared = true;
            for (name, field) in properties {
                fields.entry(name).or_default().push(field);
            }
        }
        patterned |= schema.get("patternProperties").is_some();
        if let Some(schema) = schema
            .get("additionalProperties")
            .filter(|value| value.is_object())
        {
            dictionaries.push(schema);
        }
    }
    // Dynamic map keys and arrays are replacements, not anonymous record identities.
    for (name, old) in old {
        if let Some(fields) = fields.get(name.as_str()) {
            if let Some(value) = object.get_mut(name) {
                preserve(root, fields, old, value, depth + 1)?;
            }
        } else if !dictionaries.is_empty() {
            if let Some(value) = object.get_mut(name) {
                preserve(root, &dictionaries, old, value, depth + 1)?;
            }
        } else if !patterned && declared && !object.contains_key(name) {
            object.insert(name.clone(), old.clone());
        }
    }
    Ok(())
}

fn object_schemas<'a>(
    root: &'a Value,
    schema: &'a Value,
    depth: usize,
    nodes: &mut Vec<&'a Value>,
) -> Result<(), Fault> {
    if nodes.iter().any(|node| std::ptr::eq(*node, schema)) {
        return Ok(());
    }
    if depth > storage::MAX_SCHEMA_DEPTH {
        return Err(invalid("storage normalization exceeds the depth limit"));
    }
    nodes.push(schema);
    if let Some(target) = reference(root, schema)? {
        object_schemas(root, target, depth + 1, nodes)?;
    }
    // These declarations identify owned fields without selecting a conditional
    // branch or applying any branch-specific defaults.
    for key in ["allOf", "anyOf", "oneOf"] {
        if let Some(branches) = schema.get(key).and_then(Value::as_array) {
            for branch in branches {
                object_schemas(root, branch, depth + 1, nodes)?;
            }
        }
    }
    for key in ["then", "else"] {
        if let Some(branch) = schema.get(key) {
            object_schemas(root, branch, depth + 1, nodes)?;
        }
    }
    Ok(())
}

fn check_size(value: &Value, limit: usize) -> Result<usize, Fault> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| invalid("invalid plugin JSON"))?
        .len();
    if bytes > limit {
        return Err(invalid("plugin JSON exceeds the size limit"));
    }
    Ok(bytes)
}

fn invalid(message: &str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}

#[cfg(test)]
mod tests;
