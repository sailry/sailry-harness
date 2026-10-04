//! Native search defaults verified against the provider tool documentation.
//! Compatible endpoints use the selected API contract and known model defaults.
//! Unknown models still need an explicit capability; function calling alone is insufficient.
use sailry_protocol::{
    Authentication, Effort,
    conversation::{ModelApi, Provider},
};

// Google Search + function calling is supported by Gemini 3, not Gemini 2.
// https://ai.google.dev/gemini-api/docs/google-search#supported_tool_combinations
// https://developers.openai.com/api/docs/guides/tools-web-search#limitations
pub(crate) fn combines(api: ModelApi, id: &str, effort: Effort, functions: bool) -> bool {
    !matches!(
        (api, id, effort),
        (ModelApi::Responses, "gpt-5", Effort::Minimal)
    ) && !(api == ModelApi::Gemini && id.starts_with("gemini-2.") && functions)
}

pub(crate) fn enable(provider: &mut Provider) {
    if provider.authentication != Authentication::ApiKey {
        return;
    }
    for model in &mut provider.models {
        model.web_search |= supported(provider.api, &model.id);
    }
}

pub(crate) fn supported(api: ModelApi, id: &str) -> bool {
    let models: &[&str] = match api {
        ModelApi::Responses => &[
            "gpt-4o",
            "gpt-4o-mini",
            "gpt-4.1",
            "gpt-4.1-mini",
            "o3",
            "o4-mini",
            "gpt-5",
            "gpt-5-mini",
            "gpt-5.1",
            "gpt-5.2",
            "gpt-5.4",
            "gpt-5.5",
            "gpt-5.6",
            "gpt-5.6-luna",
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-6-astra",
        ],
        ModelApi::Anthropic => &[
            "claude-sonnet-4-6",
            "claude-sonnet-4-5",
            "claude-opus-4-6",
            "claude-opus-4-7",
            "claude-opus-4-8",
            "claude-haiku-4-5",
            "claude-sonnet-4-5-20250929",
            "claude-haiku-4-5-20251001",
            "claude-sonnet-5",
            "claude-opus-5",
            "claude-fable-5-1",
        ],
        ModelApi::Gemini => &[
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            "gemini-2.5-flash-lite",
            "gemini-3-pro-preview",
            "gemini-3-flash-preview",
            "gemini-3.1-pro-preview",
            "gemini-3.1-flash-lite",
            "gemini-3.5-flash",
            "gemini-3.5-flash-lite",
            "gemini-3.6-flash",
            "gemini-3.7-flash",
            "gemini-3.8-flash",
        ],
        _ => return false,
    };
    models.contains(&id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_supported_models_and_protocols() {
        assert!(supported(ModelApi::Responses, "gpt-5.6-luna"));
        assert!(!supported(ModelApi::Responses, "gpt-image-1"));
        assert!(!supported(ModelApi::Responses, "unknown-model"));
        assert!(!supported(ModelApi::ChatCompletions, "gpt-5.6-luna"));
        assert!(supported(ModelApi::Anthropic, "claude-sonnet-4-6"));
        assert!(supported(ModelApi::Gemini, "gemini-2.5-flash"));
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
