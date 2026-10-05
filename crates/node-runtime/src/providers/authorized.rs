//! Account-scoped catalogs. Protocol references are recorded in provider-authorization.md.
use super::*;
use sailry_protocol::{Authentication, Effort};
use serde_json::Value;

// Reviewed Codex catalog compatibility baseline, independent of Sailry's version.
const CHATGPT_CATALOG_VERSION: &str = "0.160.0";

impl Discovery {
    pub(crate) async fn authorized(
        &self,
        api: ModelApi,
        endpoint: &str,
        grant: &login::Grant,
        closed: CancellationToken,
    ) -> Result<Vec<discovery::Model>, Fault> {
        self.query(fetch(api, endpoint_url(endpoint)?, grant), closed)
            .await
    }
}

async fn fetch(
    api: ModelApi,
    mut url: Url,
    grant: &login::Grant,
) -> Result<Vec<discovery::Model>, Fault> {
    url.set_path(&format!("{}/models", url.path().trim_end_matches('/')));
    let mut headers = grant.headers()?;
    match grant.authentication() {
        Authentication::ChatGpt => {
            url.query_pairs_mut()
                .append_pair("client_version", CHATGPT_CATALOG_VERSION);
        }
        Authentication::Copilot => {
            headers.insert(
                "openai-intent",
                HeaderValue::from_static("conversation-panel"),
            );
            headers.insert("x-initiator", HeaderValue::from_static("user"));
            headers.insert(
                "x-request-id",
                HeaderValue::from_str(&sailry_protocol::RequestId::new().to_string())
                    .map_err(|_| invalid("discovery request identifier is invalid"))?,
            );
        }
        Authentication::ApiKey | Authentication::Host => {
            return Err(invalid("account authorization is required"));
        }
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| unavailable("provider HTTP client failed"))?;
    let response = client
        .get(url)
        .headers(headers)
        .send()
        .await
        .map_err(|_| unavailable("provider request failed"))?;
    let mut remaining = TOTAL_BYTES;
    let value = read(response, &mut remaining).await?;
    parse(grant.authentication(), api, &value)
}

fn parse(
    authentication: Authentication,
    api: ModelApi,
    value: &Value,
) -> Result<Vec<discovery::Model>, Fault> {
    let chatgpt = authentication == Authentication::ChatGpt;
    let entries = value[if chatgpt { "models" } else { "data" }]
        .as_array()
        .ok_or_else(|| unavailable("provider model list has invalid shape"))?;
    if entries.len() > MODEL_LIMIT {
        return Err(unavailable("provider model list exceeds its limit"));
    }
    let mut models = BTreeMap::new();
    for entry in entries {
        if !chatgpt && !supports(entry, api)? {
            continue;
        }
        let id = entry[if chatgpt { "slug" } else { "id" }]
            .as_str()
            .ok_or_else(|| unavailable("provider model identifier is missing"))?;
        if id.trim().is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
            return Err(unavailable("provider model identifier is invalid"));
        }
        let (context, output, capabilities) = if chatgpt {
            let (reasoning, efforts) = efforts(entry.get("supported_reasoning_levels"), true)?;
            let default_effort = entry
                .get("default_reasoning_level")
                .filter(|value| !value.is_null())
                .map(effort)
                .transpose()?
                .flatten();
            if let (Some(default), Some(efforts)) = (default_effort, &efforts)
                && !efforts.contains(&default)
            {
                return Err(unavailable("provider reasoning default is not supported"));
            }
            let vision = entry
                .get("input_modalities")
                .filter(|value| !value.is_null())
                .map(|value| {
                    let values = strings(value)?;
                    Ok::<_, Fault>(values.contains(&"image"))
                })
                .transpose()?;
            (
                metadata::limits(entry, &["context_window"])?,
                metadata::limits(entry, &["max_output_tokens"])?,
                discovery::Capabilities {
                    vision,
                    web_search: None,
                    tools: None,
                    reasoning,
                    efforts,
                    default_effort,
                },
            )
        } else {
            let limits = &entry["capabilities"]["limits"];
            let supports = &entry["capabilities"]["supports"];
            let (reasoning, efforts) = efforts(supports.get("reasoning_effort"), false)?;
            (
                metadata::limits(limits, &["max_context_window_tokens"])?,
                metadata::limits(limits, &["max_output_tokens"])?,
                discovery::Capabilities {
                    vision: boolean(supports, "vision")?,
                    web_search: boolean(supports, "web_search")?,
                    tools: boolean(supports, "tool_calls")?,
                    reasoning,
                    efforts,
                    default_effort: None,
                },
            )
        };
        if context
            .zip(output)
            .is_some_and(|(context, output)| output > context)
        {
            return Err(unavailable("provider model limits are inconsistent"));
        }
        let model = discovery::Model {
            id: id.into(),
            context,
            output,
            capabilities: Some(capabilities),
        };
        if models
            .insert(id.to_owned(), model.clone())
            .is_some_and(|old| old != model)
        {
            return Err(unavailable("provider returned conflicting model metadata"));
        }
    }
    Ok(models.into_values().collect())
}

fn supports(entry: &Value, api: ModelApi) -> Result<bool, Fault> {
    if entry["capabilities"]["type"] != "chat" {
        return Ok(false);
    }
    let Some(endpoints) = entry
        .get("supported_endpoints")
        .filter(|value| !value.is_null())
    else {
        return Ok(api == ModelApi::ChatCompletions);
    };
    let endpoint = match api {
        ModelApi::ChatCompletions => "/chat/completions",
        ModelApi::Responses => "/responses",
        _ => return Ok(false),
    };
    Ok(strings(endpoints)?.contains(&endpoint))
}

fn strings(value: &Value) -> Result<Vec<&str>, Fault> {
    let values = value
        .as_array()
        .filter(|values| values.len() <= 32)
        .ok_or_else(|| unavailable("provider model choices are invalid"))?;
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|value| {
                    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
                })
                .ok_or_else(|| unavailable("provider model choice is invalid"))
        })
        .collect()
}

fn boolean(value: &Value, name: &str) -> Result<Option<bool>, Fault> {
    value
        .get(name)
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_bool()
                .ok_or_else(|| unavailable("provider model capability is invalid"))
        })
        .transpose()
}

pub(super) fn effort(value: &Value) -> Result<Option<Effort>, Fault> {
    let name = value
        .as_str()
        .filter(|value| {
            !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
        })
        .ok_or_else(|| unavailable("provider reasoning choice is invalid"))?;
    Ok(match name {
        "none" => Some(Effort::Disabled),
        "minimal" => Some(Effort::Minimal),
        "low" => Some(Effort::Low),
        "medium" => Some(Effort::Medium),
        "high" => Some(Effort::High),
        "xhigh" => Some(Effort::XHigh),
        "max" => Some(Effort::Max),
        _ => None,
    })
}

pub(super) fn efforts(
    value: Option<&Value>,
    objects: bool,
) -> Result<(Option<bool>, Option<Vec<Effort>>), Fault> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok((None, None));
    };
    let values = value
        .as_array()
        .filter(|values| values.len() <= 32)
        .ok_or_else(|| unavailable("provider reasoning choices are invalid"))?;
    let mut choices = Vec::new();
    for value in values {
        if let Some(choice) = effort(if objects { &value["effort"] } else { value })? {
            if choices.contains(&choice) {
                return Err(unavailable("provider reasoning choices are duplicated"));
            }
            choices.push(choice);
        }
    }
    Ok((Some(!values.is_empty()), Some(choices)))
}

#[cfg(test)]
mod tests;
