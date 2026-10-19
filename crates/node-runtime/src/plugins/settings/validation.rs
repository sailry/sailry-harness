use super::*;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn parse(
    bytes: &[u8],
    servers: &BTreeMap<String, mcp::Server>,
) -> Result<Schema, Fault> {
    if bytes.len() > MAX_BYTES {
        return Err(sailry_protocol::Fault::invalid(
            "settings schema exceeds the size limit",
        ));
    }
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| sailry_protocol::Fault::invalid("invalid settings JSON"))?;
    // The typed subset rejects external references and nested schemas before library construction.
    let schema: Schema = serde_json::from_value(value.clone())
        .map_err(|_| sailry_protocol::Fault::invalid("unsupported settings form schema"))?;
    if schema.schema != SCHEMA
        || schema.additional_properties
        || schema.properties.len() > MAX_FIELDS
        || schema.required.len() > MAX_FIELDS
        || !bounded_text(schema.title.as_deref())
        || !bounded_text(schema.description.as_deref())
    {
        return Err(sailry_protocol::Fault::invalid(
            "invalid settings form schema",
        ));
    }
    let mut required = BTreeSet::new();
    for labels in schema.locales.values() {
        for (key, value) in labels {
            if !identifier(key) || !bounded_text(Some(value)) {
                return Err(sailry_protocol::Fault::invalid(
                    "invalid settings translation",
                ));
            }
        }
    }
    for name in &schema.required {
        if !schema.properties.contains_key(name) || !required.insert(name) {
            return Err(sailry_protocol::Fault::invalid("invalid required setting"));
        }
    }
    let mut targets = BTreeSet::new();
    if schema.tabs.len() > MAX_FIELDS {
        return Err(sailry_protocol::Fault::invalid("too many settings tabs"));
    }
    let mut tab_ids = BTreeSet::new();
    let mut tab_fields = BTreeSet::new();
    for tab in &schema.tabs {
        if !identifier(&tab.id)
            || !tab_ids.insert(&tab.id)
            || tab.title.trim().is_empty()
            || !bounded_text(Some(&tab.title))
            || tab.fields.is_empty()
            || tab.fields.len() > MAX_FIELDS
            || tab
                .fields
                .iter()
                .any(|name| !schema.properties.contains_key(name) || !tab_fields.insert(name))
            || tab.enabled.as_ref().is_some_and(|name| {
                !tab.fields.contains(name)
                    || schema
                        .properties
                        .get(name)
                        .is_none_or(|field| field.kind != Kind::Boolean)
            })
        {
            return Err(sailry_protocol::Fault::invalid("invalid settings tab"));
        }
    }
    if schema
        .properties
        .values()
        .filter(|field| field.child_concurrency)
        .count()
        > 1
    {
        return Err(Fault::invalid(
            "only one child concurrency setting may be declared",
        ));
    }
    for (name, field) in &schema.properties {
        if field.child_concurrency
            && (field.kind != Kind::String
                || field.secret.is_some()
                || field.model
                || field.effort.is_some()
                || field.choices.is_some()
                || !schema.required.contains(name)
                || field
                    .default
                    .as_ref()
                    .is_none_or(|value| super::child_limit(value).is_err()))
        {
            return Err(Fault::invalid(
                "child concurrency requires a public decimal string setting with a default",
            ));
        }
        if !identifier(name)
            || !bounded_text(field.title.as_deref())
            || !bounded_text(field.description.as_deref())
        {
            return Err(sailry_protocol::Fault::invalid(
                "invalid settings field metadata",
            ));
        }
        let raw = &value["properties"][name];
        if let Some(model) = &field.effort
            && (!schema
                .properties
                .get(model)
                .is_some_and(|field| field.model)
                || field.kind != Kind::String
                || field.model
                || field.secret.is_some()
                || field.choices.is_some())
        {
            return Err(sailry_protocol::Fault::invalid(
                "reasoning settings must reference a model field",
            ));
        }
        if raw.get("x-sailry-secret").is_some() && field.secret.is_none() {
            return Err(sailry_protocol::Fault::invalid(
                "secret binding must be an object",
            ));
        }
        if let Some(binding) = &field.secret {
            if field.kind != Kind::String
                || raw.get("default").is_some()
                || raw.get("enum").is_some()
            {
                return Err(sailry_protocol::Fault::invalid(
                    "secret fields must be strings without defaults or choices",
                ));
            }
            let target = validate_binding(binding, servers)?;
            if let Some(name) = binding
                .application
                .as_ref()
                .map(|value| &value.client_id)
                .or_else(|| binding.oauth.as_ref().map(|value| &value.client_id))
                && !schema
                    .properties
                    .get(name)
                    .is_some_and(|field| field.kind == Kind::String && field.secret.is_none())
            {
                return Err(Fault::invalid(
                    "credential client ID must reference a public string setting",
                ));
            }
            if !targets.insert(target) {
                return Err(sailry_protocol::Fault::invalid(
                    "secret settings must target distinct slots",
                ));
            }
        }
        validate_field(field, raw)?;
    }
    build(&value)?;
    Ok(schema)
}

fn validate_field(field: &Field, raw: &Value) -> Result<(), Fault> {
    if field.model
        && (field.kind != Kind::String || field.secret.is_some() || field.choices.is_some())
    {
        return Err(sailry_protocol::Fault::invalid(
            "model settings must be public strings without fixed choices",
        ));
    }
    if (field.kind != Kind::String
        && (field.min_length.is_some() || field.max_length.is_some() || field.pattern.is_some()))
        || field
            .pattern
            .as_ref()
            .is_some_and(|pattern| pattern.len() > MAX_STRING)
        || (!matches!(field.kind, Kind::Integer | Kind::Number)
            && (field.minimum.is_some() || field.maximum.is_some()))
        || field
            .min_length
            .zip(field.max_length)
            .is_some_and(|(min, max)| min > max)
        || field
            .min_length
            .into_iter()
            .chain(field.max_length)
            .any(|length| length > MAX_STRING)
        || field.choices.as_ref().is_some_and(|choices| {
            choices.is_empty()
                || choices.len() > MAX_CHOICES
                || choices
                    .iter()
                    .enumerate()
                    .any(|(index, value)| choices[..index].contains(value))
        })
    {
        return Err(sailry_protocol::Fault::invalid(
            "invalid settings field constraints",
        ));
    }
    if let (Some(min), Some(max)) = (&field.minimum, &field.maximum)
        && !build(&json!({"maximum": max}))?.is_valid(&Value::Number(min.clone()))
    {
        return Err(sailry_protocol::Fault::invalid(
            "settings minimum exceeds maximum",
        ));
    }
    let validator = build(raw)?;
    for value in raw
        .get("default")
        .into_iter()
        .chain(field.choices.iter().flatten())
    {
        if value
            .as_str()
            .is_some_and(|text| text.chars().count() > MAX_STRING)
            || !validator.is_valid(value)
        {
            return Err(sailry_protocol::Fault::invalid(
                "invalid settings default or choice",
            ));
        }
    }
    Ok(())
}

fn validate_binding(
    binding: &Binding,
    servers: &BTreeMap<String, mcp::Server>,
) -> Result<(String, String), Fault> {
    if binding.unsupported {
        return Err(sailry_protocol::Fault::invalid(
            "unsupported protected setting binding",
        ));
    }
    if binding.oauth.is_some() {
        let name = binding
            .server
            .as_deref()
            .ok_or_else(|| Fault::invalid("OAuth credentials require an MCP server"))?;
        if binding.env.is_some()
            || binding.header.is_some()
            || binding.origin.is_some()
            || binding.prefix.is_some()
            || binding.application.is_some()
            || !matches!(
                servers.get(name),
                Some(mcp::Server::Http { .. } | mcp::Server::Sse { .. })
            )
        {
            return Err(Fault::invalid(
                "OAuth client secrets require one HTTP MCP server",
            ));
        }
        return Ok((name.into(), "oauth".into()));
    }
    if let Some(application) = &binding.application {
        super::super::applications::validate(application.provider, binding, servers)?;
    }
    if binding.origin.is_some() {
        let (origin, header, _) = super::super::http::binding(binding)?;
        return Ok((origin.origin().ascii_serialization(), header.to_string()));
    }
    if binding.prefix.is_some() && (binding.header.is_none() || binding.env.is_some()) {
        return Err(sailry_protocol::Fault::invalid(
            "secret prefix requires an HTTP header slot",
        ));
    }
    let name = binding
        .server
        .as_deref()
        .ok_or_else(|| sailry_protocol::Fault::invalid("secret setting requires a target"))?;
    let server = servers.get(name).ok_or_else(|| {
        sailry_protocol::Fault::invalid("secret setting references an unknown MCP server")
    })?;
    let slot = match (server, &binding.env, &binding.header) {
        (mcp::Server::Stdio { env, .. }, Some(name), None)
            if identifier(name)
                && !["SAILRY_PLUGIN_SETTINGS_FILE", "PLUGIN_ROOT", "PLUGIN_DATA"]
                    .iter()
                    .any(|reserved| name.eq_ignore_ascii_case(reserved))
                && env.get(name).is_some_and(String::is_empty) =>
        {
            // Windows environment keys are case-insensitive; keep the contract portable.
            format!("env:{}", name.to_ascii_uppercase())
        }
        (
            mcp::Server::Http { headers, .. } | mcp::Server::Sse { headers, .. },
            None,
            Some(name),
        ) => {
            if mcp::protocol_header(name) {
                return Err(sailry_protocol::Fault::invalid(
                    "secret binding targets a protocol-controlled header",
                ));
            }
            let header = reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| sailry_protocol::Fault::invalid("invalid secret header name"))?;
            if binding
                .prefix
                .as_ref()
                .is_some_and(|prefix| reqwest::header::HeaderValue::from_str(prefix).is_err())
            {
                return Err(sailry_protocol::Fault::invalid(
                    "invalid MCP credential prefix",
                ));
            }
            if !headers
                .iter()
                .any(|(key, value)| key.eq_ignore_ascii_case(name) && value.is_empty())
            {
                return Err(sailry_protocol::Fault::invalid(
                    "secret header must have an empty MCP declaration",
                ));
            }
            format!("header:{header}")
        }
        _ => {
            return Err(sailry_protocol::Fault::invalid(
                "secret binding must reference one empty supported MCP slot",
            ));
        }
    };
    Ok((name.into(), slot))
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn bounded_text(value: Option<&str>) -> bool {
    value.is_none_or(|value| value.len() <= MAX_STRING && !value.contains('\0'))
}

pub(super) fn build(value: &Value) -> Result<jsonschema::Validator, Fault> {
    jsonschema::draft202012::options()
        .build(value)
        .map_err(|_| sailry_protocol::Fault::invalid("invalid settings schema constraints"))
}
