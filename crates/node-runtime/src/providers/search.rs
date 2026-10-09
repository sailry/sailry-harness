//! Native search defaults verified against the provider tool documentation.
//! Compatible endpoints use the selected API contract and known model defaults.
//! Codex supplies its hosted search contract independently of model identifiers.
use sailry_protocol::{
    Authentication, Effort,
    conversation::{ModelApi, Provider},
};

// Google Search + function calling is supported by Gemini 3, not Gemini 2.
// https://ai.google.dev/gemini-api/docs/google-search#supported_tool_combinations
// https://developers.openai.com/api/docs/guides/tools-web-search#limitations
pub(crate) fn combines(api: ModelApi, id: &str, effort: Effort, functions: bool) -> bool {
    !(api == ModelApi::Responses && initial_gpt5(id) && effort == Effort::Minimal)
        && !(api == ModelApi::Gemini && id.starts_with("gemini-2.") && functions)
}

pub(crate) fn enable(provider: &mut Provider) {
    let codex = provider.authentication == Authentication::ChatGptOAuth
        && provider.api == ModelApi::Responses;
    if provider.authentication != Authentication::ApiKey && !codex {
        return;
    }
    for model in &mut provider.models {
        model.web_search = model
            .capabilities
            .as_ref()
            .and_then(|value| value.web_search)
            // Codex's Responses provider declares hosted web search without a model allowlist.
            // https://github.com/openai/codex/blob/e45069d770c6d0b8984c58a6e7cbaf221ad37599/codex-rs/core/src/tools/spec_plan.rs
            .unwrap_or_else(|| codex || model.web_search || supported(provider.api, &model.id));
    }
}

pub(crate) fn supported(api: ModelApi, id: &str) -> bool {
    if [
        "image",
        "audio",
        "realtime",
        "transcribe",
        "tts",
        "embedding",
    ]
    .iter()
    .any(|kind| id.split('-').any(|part| part == *kind))
    {
        return false;
    }
    match api {
        ModelApi::Responses => {
            if let Some(id) = id.strip_prefix("gpt-") {
                return generation(id).is_some_and(|(major, minor)| {
                    major >= 5
                        || major == 4 && minor >= 1 && !id.split('-').any(|part| part == "nano")
                }) || id.strip_prefix("4o").is_some_and(boundary);
            }
            id.strip_prefix('o')
                .and_then(generation)
                .is_some_and(|(major, _)| major >= 3)
        }
        ModelApi::Anthropic => id
            .strip_prefix("claude-")
            .and_then(|id| id.split_once('-'))
            .and_then(|(_, version)| generation(version))
            .is_some_and(|(major, _)| major >= 4),
        ModelApi::Gemini => id
            .strip_prefix("gemini-")
            .and_then(generation)
            .is_some_and(|(major, _)| major >= 2),
        _ => false,
    }
}

fn boundary(suffix: &str) -> bool {
    suffix.is_empty() || suffix.starts_with('-')
}

fn initial_gpt5(id: &str) -> bool {
    id == "gpt-5"
        || id.strip_prefix("gpt-5-").is_some_and(|snapshot| {
            snapshot.len() == 10
                && snapshot.bytes().enumerate().all(|(index, byte)| {
                    if matches!(index, 4 | 7) {
                        byte == b'-'
                    } else {
                        byte.is_ascii_digit()
                    }
                })
        })
}

fn generation(id: &str) -> Option<(u16, u16)> {
    let version = id.split('-').next()?;
    let (major, minor) = version.split_once('.').unwrap_or((version, "0"));
    Some((major.parse().ok()?, minor.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider(authentication: Authentication, api: ModelApi) -> Provider {
        serde_json::from_value(json!({
            "id":sailry_protocol::ProviderId::new(), "revision":0, "name":"Search fixture",
            "api":api, "authentication":authentication, "endpoint":"https://example.com",
            "enabled":true, "default_model":"catalog-next", "credential":null,
            "models":[{"id":"catalog-next", "context":4096, "output":512}]
        }))
        .unwrap()
    }

    #[test]
    fn codex_contract_ignores_model_names() {
        let mut provider = provider(Authentication::ChatGptOAuth, ModelApi::Responses);
        for id in ["catalog-next", "vendor-family-preview", "unknown-revision"] {
            provider.models[0].id = id.into();
            provider.models[0].web_search = false;
            enable(&mut provider);
            assert!(provider.models[0].web_search, "{id}");
        }
        provider.models[0].capabilities =
            Some(sailry_protocol::conversation::discovery::Capabilities {
                web_search: Some(false),
                ..Default::default()
            });
        enable(&mut provider);
        assert!(!provider.models[0].web_search);
    }

    #[test]
    fn confines_defaults_to_supported_authentication() {
        for (authentication, api) in [
            (Authentication::ChatGptOAuth, ModelApi::ChatCompletions),
            (Authentication::ChatGpt, ModelApi::Responses),
            (Authentication::Copilot, ModelApi::Responses),
            (Authentication::ApiKey, ModelApi::Responses),
        ] {
            let mut provider = provider(authentication, api);
            enable(&mut provider);
            assert!(
                !provider.models[0].web_search,
                "{authentication:?}, {api:?}"
            );
        }
    }
    #[test]
    fn matches_supported_models_and_protocols() {
        assert!(supported(ModelApi::Responses, "gpt-5.6-luna"));
        assert!(!supported(ModelApi::Responses, "gpt-image-1"));
        assert!(!supported(ModelApi::Responses, "unknown-model"));
        assert!(!supported(ModelApi::ChatCompletions, "gpt-5.6-luna"));
        assert!(supported(ModelApi::Anthropic, "claude-sonnet-4-6"));
        assert!(supported(ModelApi::Gemini, "gemini-2.5-flash"));
        assert!(supported(ModelApi::Gemini, "gemini-2.0-flash"));
        assert!(!supported(ModelApi::Gemini, "gemini-1.5-flash"));
        for model in ["gpt-6.1-sol", "gpt-6.1-sol-20261001", "gpt-5.6-sol"] {
            assert!(supported(ModelApi::Responses, model), "{model}");
        }
        for model in ["claude-sonnet-5-5", "claude-opus-5-5", "claude-haiku-5-5"] {
            assert!(supported(ModelApi::Anthropic, model), "{model}");
        }
        for model in ["gpt-4.1-nano", "gpt-6-image", "gpt-6-audio", "gpt-6invalid"] {
            assert!(!supported(ModelApi::Responses, model), "{model}");
        }
    }

    #[test]
    fn respects_tool_combinations() {
        assert!(!combines(
            ModelApi::Responses,
            "gpt-5",
            Effort::Minimal,
            false
        ));
        assert!(combines(ModelApi::Responses, "gpt-5", Effort::Low, true));
        assert!(!combines(
            ModelApi::Responses,
            "gpt-5-2025-08-07",
            Effort::Minimal,
            true
        ));
        for id in ["gpt-5-mini", "gpt-5-nano", "gpt-5.1", "gpt-5-vendor-alias"] {
            assert!(
                combines(ModelApi::Responses, id, Effort::Minimal, true),
                "{id}"
            );
        }
        assert!(combines(
            ModelApi::Gemini,
            "gemini-2.0-flash",
            Effort::Default,
            false
        ));
        assert!(!combines(
            ModelApi::Gemini,
            "gemini-2.0-flash",
            Effort::Default,
            true
        ));
        assert!(!combines(
            ModelApi::Gemini,
            "gemini-2.5-flash",
            Effort::Default,
            true
        ));
        assert!(combines(
            ModelApi::Gemini,
            "gemini-2.5-flash",
            Effort::Default,
            false
        ));
        assert!(combines(
            ModelApi::Gemini,
            "gemini-3.5-flash",
            Effort::Default,
            true
        ));
    }
}
