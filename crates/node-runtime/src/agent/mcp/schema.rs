//! Adapt the pinned MCP primitive form contract to shared controller fields.
use super::*;
use sailry_protocol::conversation::question::{
    Input, Spec,
    form::{Choice, Field, Input as FieldInput},
};

pub(super) fn spec(
    message: String,
    schema: &Value,
    order: Option<&[String]>,
) -> Result<Spec, rmcp::ErrorData> {
    let properties = schema["properties"].as_object().ok_or_else(invalid)?;
    let required = schema["required"].as_array().cloned().unwrap_or_default();
    if required.iter().any(|name| {
        name.as_str()
            .is_none_or(|name| !properties.contains_key(name))
    }) {
        return Err(invalid());
    }
    let mut names = Vec::new();
    for name in order
        .into_iter()
        .flatten()
        .map(String::as_str)
        .chain(properties.keys().map(String::as_str))
    {
        if !properties.contains_key(name) {
            return Err(invalid());
        }
        if !names.contains(&name) {
            names.push(name);
        }
    }
    let fields = names
        .into_iter()
        .map(|name| {
            let property = &properties[name];
            let input = match property["type"].as_str() {
                Some("string")
                    if property.get("enum").is_some() || property.get("oneOf").is_some() =>
                {
                    FieldInput::Choice {
                        options: choices(property, "oneOf")?,
                        multiple: false,
                        min_items: None,
                        max_items: None,
                    }
                }
                Some("string") => FieldInput::Text {
                    min_length: property["minLength"].as_u64(),
                    max_length: property["maxLength"].as_u64(),
                    format: property["format"].as_str().map(str::to_owned),
                },
                Some("number" | "integer") => FieldInput::Number {
                    integer: property["type"] == "integer",
                    minimum: property["minimum"].as_number().cloned(),
                    maximum: property["maximum"].as_number().cloned(),
                },
                Some("boolean") => FieldInput::Boolean,
                Some("array") => FieldInput::Choice {
                    options: choices(&property["items"], "anyOf")?,
                    multiple: true,
                    min_items: property["minItems"].as_u64(),
                    max_items: property["maxItems"].as_u64(),
                },
                _ => return Err(invalid()),
            };
            Ok(Field {
                name: name.into(),
                title: property["title"].as_str().unwrap_or(name).into(),
                description: property["description"].as_str().map(str::to_owned),
                required: required.iter().any(|value| value.as_str() == Some(name)),
                input,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let spec = Spec {
        prompt: message,
        input: Input::Form { fields },
    };
    spec.validate().map_err(|_| invalid())?;
    jsonschema::draft202012::options()
        .should_validate_formats(true)
        .build(schema)
        .map_err(|_| invalid())?;
    Ok(spec)
}

fn choices(property: &Value, titled: &str) -> Result<Vec<Choice>, rmcp::ErrorData> {
    if let Some(values) = property["enum"].as_array() {
        return values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let value = value.as_str().ok_or_else(invalid)?;
                Ok(Choice {
                    value: value.into(),
                    title: property["enumNames"][index]
                        .as_str()
                        .unwrap_or(value)
                        .into(),
                })
            })
            .collect();
    }
    property[titled]
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .map(|option| {
            Ok(Choice {
                value: option["const"].as_str().ok_or_else(invalid)?.into(),
                title: option["title"].as_str().ok_or_else(invalid)?.into(),
            })
        })
        .collect()
}

fn invalid() -> rmcp::ErrorData {
    rmcp::ErrorData::invalid_params("unsupported or invalid MCP form", None)
}
