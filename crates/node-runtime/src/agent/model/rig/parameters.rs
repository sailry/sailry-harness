//! Request policy derived from Goose 14741e9a provider formats.
//! Apache-2.0; see third_party_licenses/goose-Apache-2.0.txt.
use super::super::types::Request;
use rig_core::catalog::Catalog;
use sailry_protocol::{Effort, conversation::ModelApi};
use serde_json::{Value, json};

#[cfg(test)]
mod tests;

pub(super) fn prepare(request: &mut Request, api: ModelApi, configured: ModelApi) {
    match api {
        ModelApi::Responses | ModelApi::ChatCompletions | ModelApi::DeepSeek => {
            openai(request, api, configured);
            if !matches!(configured, ModelApi::OpenCodeGo | ModelApi::OpenCodeZen) {
                request.params.remove("budget_tokens");
            }
        }
        ModelApi::Anthropic => {
            anthropic(request);
            request.params.remove("budget_tokens");
        }
        ModelApi::Gemini => {
            if let Some(thinking) = gemini(request) {
                let generation = request
                    .params
                    .entry("generationConfig")
                    .or_insert_with(|| json!({}));
                if let Some(generation) = generation.as_object_mut() {
                    generation.entry("thinkingConfig").or_insert(thinking);
                }
            }
            request.params.remove("thinking_budget");
        }
        _ => {}
    }
    request.effort = Effort::Default;
}

pub(super) fn openai_reasoning(request: &Request) -> bool {
    super::super::context::family(&request.model) == Some("openai") && reasoning(request)
}

fn reasoning(request: &Request) -> bool {
    request.reasoning.unwrap_or_else(|| {
        Catalog::builtin()
            .resolve(&format!(
                "openai/{}",
                request.model.rsplit('/').next().unwrap_or(&request.model)
            ))
            .map(|resolved| resolved.spec.reasoning.supported())
            .unwrap_or_else(|_| super::cloud::responses_model(&request.model))
    })
}

fn openai(request: &mut Request, api: ModelApi, configured: ModelApi) {
    let openai = openai_reasoning(request);
    if openai || configured == ModelApi::AzureAi && reasoning(request) {
        request.temperature = None;
    }
    if request.reasoning != Some(false)
        && let Some(effort) = effort(request.effort)
    {
        request
            .params
            .entry("reasoning_effort")
            .or_insert_with(|| effort.into());
    }
    if api == ModelApi::Responses {
        if reasoning(request) {
            request
                .params
                .entry("include")
                .or_insert_with(|| json!(["reasoning.encrypted_content"]));
        }
        if let Some(effort) = request.params.remove("reasoning_effort") {
            let value = request
                .params
                .entry("reasoning")
                .or_insert_with(|| json!({}));
            if let Some(value) = value.as_object_mut() {
                value.entry("effort").or_insert(effort);
                value.entry("summary").or_insert_with(|| "auto".into());
            }
        }
    } else if let Some(tokens) = request.max_tokens.take() {
        let field = if openai && api != ModelApi::DeepSeek && configured != ModelApi::AzureAi {
            "max_completion_tokens"
        } else {
            "max_tokens"
        };
        request.params.entry(field).or_insert_with(|| tokens.into());
    }
}

fn anthropic(request: &mut Request) {
    let output = *request.max_tokens.get_or_insert(4096);
    let resolved = Catalog::builtin().resolve(&format!("anthropic/{}", request.model));
    let spec = resolved.as_ref().ok().map(|resolved| resolved.spec);
    let adaptive = spec.is_some_and(|spec| spec.compat.adaptive_thinking);
    let always_on =
        adaptive && spec.is_some_and(|spec| spec.reasoning.can_disable() == Some(false));
    let reasoning = request
        .reasoning
        .unwrap_or_else(|| spec.is_some_and(|spec| spec.reasoning.supported()));
    let enabled = reasoning
        && (always_on
            || !matches!(request.effort, Effort::Default | Effort::Disabled)
            || request.effort == Effort::Default && request.params.contains_key("budget_tokens"));
    if enabled && adaptive {
        request
            .params
            .entry("thinking")
            .or_insert_with(|| json!({"type":"adaptive"}));
        let output = request
            .params
            .entry("output_config")
            .or_insert_with(|| json!({}));
        if let Some(output) = output.as_object_mut() {
            output.entry("effort").or_insert_with(|| {
                match request.effort {
                    Effort::Default | Effort::Disabled | Effort::Budget(_) => "high",
                    selected => effort(selected).unwrap_or("high"),
                }
                .into()
            });
        }
    } else if enabled {
        let budget = request
            .params
            .get("budget_tokens")
            .and_then(Value::as_i64)
            .unwrap_or_else(|| match request.effort {
                Effort::Budget(tokens) => i64::from(tokens),
                Effort::Minimal | Effort::Low => 4000,
                Effort::Medium => 10000,
                Effort::XHigh | Effort::Max => 32000,
                _ => 16000,
            })
            .max(1024) as u64;
        let budget = budget.min(output.saturating_sub(1024));
        if budget >= 1024 {
            request
                .params
                .entry("thinking")
                .or_insert_with(|| json!({"type":"enabled","budget_tokens":budget}));
        }
    } else if adaptive && !always_on {
        request
            .params
            .entry("thinking")
            .or_insert_with(|| json!({"type":"disabled"}));
    }
    if request.model.to_ascii_lowercase().contains("claude")
        && let Some(thinking) = request
            .params
            .get_mut("thinking")
            .and_then(Value::as_object_mut)
        && matches!(
            thinking.get("type").and_then(Value::as_str),
            Some("enabled" | "adaptive")
        )
    {
        thinking
            .entry("display")
            .or_insert_with(|| "summarized".into());
    }
}

fn effort(effort: Effort) -> Option<&'static str> {
    match effort {
        Effort::Default | Effort::Budget(_) => None,
        Effort::Disabled => Some("none"),
        Effort::Minimal => Some("minimal"),
        Effort::Low => Some("low"),
        Effort::Medium => Some("medium"),
        Effort::High => Some("high"),
        Effort::XHigh => Some("xhigh"),
        Effort::Max => Some("max"),
    }
}

fn gemini(request: &Request) -> Option<Value> {
    let model = request.model.to_ascii_lowercase();
    if request.reasoning == Some(false) || request.effort == Effort::Disabled {
        return if model.starts_with("gemini-3.5") || model.starts_with("gemini-3.6") {
            Some(json!({"thinkingLevel":"minimal","includeThoughts":false}))
        } else if model.starts_with("gemini-2.5-flash") {
            Some(json!({"thinkingBudget":0,"includeThoughts":false}))
        } else {
            None
        };
    }
    if model.starts_with("gemini-3") {
        let level = match request.effort {
            Effort::Default | Effort::Disabled | Effort::Budget(_) => return None,
            Effort::Minimal | Effort::Low => "low",
            Effort::Medium if model.starts_with("gemini-3-pro") => "low",
            Effort::Medium => "medium",
            Effort::High | Effort::XHigh | Effort::Max => "high",
        };
        Some(json!({"thinkingLevel":level,"includeThoughts":true}))
    } else if model.starts_with("gemini-2.5") {
        let budget = request
            .params
            .get("thinking_budget")
            .and_then(Value::as_i64)
            .or_else(|| match request.effort {
                Effort::Budget(tokens) => Some(i64::from(tokens)),
                _ => None,
            })
            .filter(|tokens| *tokens >= 0)
            .unwrap_or(8192);
        Some(json!({"thinkingBudget":budget,"includeThoughts":true}))
    } else {
        None
    }
}
