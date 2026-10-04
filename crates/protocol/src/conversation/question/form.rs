//! Flat forms shared by controller drafts and authoritative Node validation.
use super::*;
use serde_json::{Map, Number, Value};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub name: String,
    pub title: String,
    pub description: Option<String>,
    pub required: bool,
    pub input: Input,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Input {
    Text {
        min_length: Option<u64>,
        max_length: Option<u64>,
        format: Option<String>,
    },
    Number {
        integer: bool,
        minimum: Option<Number>,
        maximum: Option<Number>,
    },
    Boolean,
    Choice {
        options: Vec<Choice>,
        multiple: bool,
        min_items: Option<u64>,
        max_items: Option<u64>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub value: String,
    pub title: String,
}

pub(super) fn validate(fields: &[Field]) -> Result<(), Fault> {
    let mut names = BTreeSet::new();
    if fields.len() > 64
        || serde_json::to_vec(fields)
            .map_err(|_| invalid("invalid form"))?
            .len()
            > MAX_TEXT_BYTES
    {
        return Err(invalid("form exceeds the supported size"));
    }
    for field in fields {
        if field.name.is_empty()
            || field.name.len() > 1024
            || !names.insert(&field.name)
            || field.title.trim().is_empty()
            || field.title.len() > 1024
        {
            return Err(invalid("form field is empty, repeated or too large"));
        }
        match &field.input {
            Input::Text {
                min_length,
                max_length,
                ..
            } if reversed(*min_length, *max_length) => return Err(invalid("invalid text bounds")),
            Input::Number {
                minimum: Some(minimum),
                maximum: Some(maximum),
                ..
            } if minimum.as_f64() > maximum.as_f64() => {
                return Err(invalid("invalid number bounds"));
            }
            Input::Choice {
                options,
                min_items,
                max_items,
                ..
            } => {
                let mut values = BTreeSet::new();
                if options.is_empty()
                    || options.len() > 64
                    || reversed(*min_items, *max_items)
                    || options.iter().any(|option| {
                        option.title.trim().is_empty() || !values.insert(&option.value)
                    })
                {
                    return Err(invalid("invalid form choices"));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn reversed(minimum: Option<u64>, maximum: Option<u64>) -> bool {
    matches!((minimum, maximum), (Some(minimum), Some(maximum)) if minimum > maximum)
}

pub(super) fn validate_answer(fields: &[Field], values: &Map<String, Value>) -> Result<(), Fault> {
    if values
        .keys()
        .any(|name| !fields.iter().any(|field| &field.name == name))
        || serde_json::to_vec(values)
            .map_err(|_| invalid("invalid form answer"))?
            .len()
            > MAX_TEXT_BYTES
    {
        return Err(invalid(
            "form answer has unknown fields or exceeds the size limit",
        ));
    }
    for field in fields {
        match values.get(&field.name) {
            None if field.required => return Err(invalid("required form field is missing")),
            None => {}
            Some(value) => field.validate_value(value)?,
        }
    }
    Ok(())
}

impl Field {
    fn validate_value(&self, value: &Value) -> Result<(), Fault> {
        let valid = match (&self.input, value) {
            (
                Input::Text {
                    min_length,
                    max_length,
                    ..
                },
                Value::String(text),
            ) => {
                let length = text.chars().count() as u64;
                text.len() <= MAX_TEXT_BYTES
                    && min_length.is_none_or(|min| length >= min)
                    && max_length.is_none_or(|max| length <= max)
            }
            (
                Input::Number {
                    integer,
                    minimum,
                    maximum,
                },
                Value::Number(number),
            ) => number.as_f64().is_some_and(|value| {
                value.is_finite()
                    && (!integer || value.fract() == 0.)
                    && minimum
                        .as_ref()
                        .and_then(Number::as_f64)
                        .is_none_or(|min| value >= min)
                    && maximum
                        .as_ref()
                        .and_then(Number::as_f64)
                        .is_none_or(|max| value <= max)
            }),
            (Input::Boolean, Value::Bool(_)) => true,
            (
                Input::Choice {
                    options,
                    multiple: false,
                    ..
                },
                Value::String(value),
            ) => options.iter().any(|option| &option.value == value),
            (
                Input::Choice {
                    options,
                    multiple: true,
                    min_items,
                    max_items,
                },
                Value::Array(values),
            ) => {
                let mut seen = BTreeSet::new();
                min_items.is_none_or(|min| values.len() as u64 >= min)
                    && max_items.is_none_or(|max| values.len() as u64 <= max)
                    && values.iter().all(|value| {
                        value.as_str().is_some_and(|value| {
                            seen.insert(value) && options.iter().any(|option| option.value == value)
                        })
                    })
            }
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(invalid("value does not match the form field"))
        }
    }
}
