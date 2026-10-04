use super::*;
use sailry_protocol::conversation::Provider;
use serde_json::Value;

pub(super) struct Page {
    pub models: Vec<discovery::Model>,
    pub next: Option<String>,
}

pub(super) fn parse(api: ModelApi, value: &Value) -> Result<Page, Fault> {
    let gemini = api == ModelApi::Gemini;
    let entries = value[if gemini { "models" } else { "data" }]
        .as_array()
        .ok_or_else(|| unavailable("provider model list has invalid shape"))?;
    let mut models = Vec::new();
    for entry in entries {
        if gemini
            && !entry["supportedGenerationMethods"]
                .as_array()
                .is_some_and(|methods| methods.iter().any(|method| method == "generateContent"))
        {
            continue;
        }
        let id = entry[if gemini { "name" } else { "id" }]
            .as_str()
            .ok_or_else(|| unavailable("provider model identifier is missing"))?;
        let id = if gemini {
            id.strip_prefix("models/").unwrap_or(id)
        } else {
            id
        };
        if id.trim().is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
            return Err(unavailable("provider model identifier is invalid"));
        }
        let (context, output) = match api {
            ModelApi::Gemini => (
                limits(entry, &["inputTokenLimit"])?,
                limits(entry, &["outputTokenLimit"])?,
            ),
            ModelApi::Anthropic => (
                limits(
                    entry,
                    &[
                        "max_input_tokens",
                        "context_window_tokens",
                        "context_window",
                        "context_length",
                    ],
                )?,
                limits(
                    entry,
                    &["max_tokens", "max_output_tokens", "max_completion_tokens"],
                )?,
            ),
            _ => (
                limits(
                    entry,
                    &["context_window_tokens", "context_window", "context_length"],
                )?,
                limits(entry, &["max_output_tokens", "max_completion_tokens"])?,
            ),
        };
        models.push(discovery::Model {
            id: id.to_owned(),
            context,
            output,
            capabilities: capabilities(entry)?,
        });
    }
    let next = match api {
        ModelApi::Gemini => {
            optional_string(value, "nextPageToken")?.filter(|value| !value.is_empty())
        }
        ModelApi::Anthropic => match value.get("has_more") {
            Some(Value::Bool(true)) => Some(
                optional_string(value, "last_id")?
                    .ok_or_else(|| unavailable("provider pagination cursor is missing"))?,
            ),
            Some(Value::Bool(false)) => None,
            // Compatible gateways may return a complete OpenAI-style catalog.
            None if value["object"] == "list" => None,
            _ => return Err(unavailable("provider pagination state is invalid")),
        },
        _ => None,
    };
    Ok(Page { models, next })
}

fn capabilities(entry: &Value) -> Result<Option<discovery::Capabilities>, Fault> {
    let boolean = |name: &str| -> Result<Option<bool>, Fault> {
        entry
            .get(name)
            .or_else(|| entry.get("capabilities").and_then(|value| value.get(name)))
            .filter(|value| !value.is_null())
            .map(|value| {
                value
                    .as_bool()
                    .ok_or_else(|| unavailable("provider model capability is invalid"))
            })
            .transpose()
    };
    let (reasoning, efforts) =
        super::authorized::efforts(entry.get("supported_reasoning_levels"), true)?;
    let default_effort = entry
        .get("default_reasoning_level")
        .filter(|value| !value.is_null())
        .map(super::authorized::effort)
        .transpose()?
        .flatten();
    if default_effort.is_some_and(|effort| {
        efforts
            .as_ref()
            .is_some_and(|values| !values.contains(&effort))
    }) {
        return Err(unavailable(
            "provider default reasoning choice is unsupported",
        ));
    }
    let value = discovery::Capabilities {
        vision: boolean("vision")?,
        tools: boolean("tool_call")?,
        reasoning: boolean("reasoning")?.or(reasoning),
        efforts,
        default_effort,
        web_search: boolean("web_search")?,
    };
    Ok((value != discovery::Capabilities::default()).then_some(value))
}

fn optional_string(value: &Value, key: &str) -> Result<Option<String>, Fault> {
    match value.get(key).filter(|value| !value.is_null()) {
        None => Ok(None),
        Some(value) => value
            .as_str()
            .map(|value| Some(value.to_owned()))
            .ok_or_else(|| unavailable("provider pagination cursor is invalid")),
    }
}

pub(super) fn limits(entry: &Value, names: &[&str]) -> Result<Option<u32>, Fault> {
    let mut found = None;
    for name in names {
        if let Some(value) = entry.get(*name).filter(|value| !value.is_null()) {
            let value = value
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| *value > 0)
                .ok_or_else(|| unavailable("provider returned invalid model limits"))?;
            if found.is_some_and(|old| old != value) {
                return Err(unavailable("provider returned conflicting model limits"));
            }
            found = Some(value);
        }
    }
    Ok(found)
}

pub(crate) fn validate(provider: &Provider, models: &[discovery::Model]) -> discovery::Validation {
    let mut result = discovery::Validation {
        provider: provider.id,
        revision: provider.revision,
        missing: vec![],
        exceeded: vec![],
        unverified: vec![],
    };
    for saved in &provider.models {
        let Some(model) = models.iter().find(|model| model.id == saved.id) else {
            result.missing.push(saved.id.clone());
            continue;
        };
        if model.context.is_some_and(|context| saved.context > context)
            || model.output.is_some_and(|output| saved.output > output)
        {
            result.exceeded.push(saved.id.clone());
        } else if model.context.is_none() || model.output.is_none() {
            result.unverified.push(saved.id.clone());
        }
    }
    result
}

#[cfg(test)]
mod capability_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserves_native_reasoning() {
        let value = capabilities(&json!({"id":"any-model", "supported_reasoning_levels":[
            {"effort":"low"}, {"effort":"xhigh"}, {"effort":"max"}], "default_reasoning_level":"xhigh"})).unwrap().unwrap();
        assert_eq!(
            value.efforts,
            Some(vec![
                sailry_protocol::Effort::Low,
                sailry_protocol::Effort::XHigh,
                sailry_protocol::Effort::Max
            ])
        );
        assert_eq!(value.default_effort, Some(sailry_protocol::Effort::XHigh));
        assert_eq!(value.reasoning, Some(true));
        assert!(capabilities(&json!({"supported_reasoning_levels":[{"effort":"high"}], "default_reasoning_level":"max"})).is_err());
        assert_eq!(
            capabilities(&json!({"supported_reasoning_levels":[]}))
                .unwrap()
                .unwrap()
                .efforts,
            Some(vec![])
        );
    }

    #[test]
    fn requires_search_metadata() {
        assert_eq!(capabilities(&json!({"id":"gpt-5.6-luna"})).unwrap(), None);
        assert_eq!(
            capabilities(&json!({"tool_call":true}))
                .unwrap()
                .unwrap()
                .web_search,
            None
        );
        for entry in [
            json!({"web_search":true}),
            json!({"capabilities":{"web_search":true}}),
        ] {
            assert_eq!(
                capabilities(&entry).unwrap().unwrap().web_search,
                Some(true)
            );
        }
        assert_eq!(
            capabilities(&json!({"web_search":false}))
                .unwrap()
                .unwrap()
                .web_search,
            Some(false)
        );
        assert!(capabilities(&json!({"web_search":"true"})).is_err());
    }
}
