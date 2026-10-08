//! Media routes reflect provider contracts, never an alternative account or model.
use sailry_protocol::{
    Authentication,
    conversation::{Model, ModelApi, Provider},
    media::{Generation, Kind},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImageRoute {
    Images,
    Responses,
    Native,
    Authorized,
}

pub(crate) fn image_route(provider: &Provider, model: &Model) -> Option<ImageRoute> {
    let route = image_api(provider)?;
    if route == ImageRoute::Responses && model.generates.contains(&Generation::Image) {
        Some(ImageRoute::Images)
    } else {
        Some(route)
    }
}

/// Select an attempt from the configured API when native model metadata is absent.
pub(crate) fn image_api(provider: &Provider) -> Option<ImageRoute> {
    if !provider.enabled {
        return None;
    }
    match provider.authentication {
        Authentication::ApiKey => match provider.api {
            ModelApi::ChatCompletions => Some(ImageRoute::Images),
            ModelApi::Responses => Some(ImageRoute::Responses),
            ModelApi::Gemini => Some(ImageRoute::Native),
            // These adapters do not share generic bearer-authenticated media
            // routing. Cloud deployment, Messages and OpenCode
            // conversation contracts must not be sent to /images/generations.
            ModelApi::Anthropic
            | ModelApi::DeepSeek
            | ModelApi::OpenCodeGo
            | ModelApi::OpenCodeZen
            | ModelApi::AzureOpenAi
            | ModelApi::AzureAi => None,
        },
        Authentication::ChatGptOAuth | Authentication::Copilot => {
            (provider.api == ModelApi::Responses).then_some(ImageRoute::Authorized)
        }
        Authentication::ChatGpt => None,
    }
}

pub(crate) fn supports(kind: Kind, provider: &Provider, model: &Model) -> bool {
    supports_api(kind, provider)
        // Model summaries are normalized before catalog selection and admission.
        && (kind != Kind::Vision || model.vision)
}

/// Check the selected API and authentication contract without model observations.
pub(crate) fn supports_api(kind: Kind, provider: &Provider) -> bool {
    if !provider.enabled {
        return false;
    }
    match kind {
        Kind::Vision => match provider.authentication {
            Authentication::ApiKey => true,
            Authentication::ChatGpt | Authentication::ChatGptOAuth => {
                provider.api == ModelApi::Responses
            }
            Authentication::Copilot => matches!(
                provider.api,
                ModelApi::ChatCompletions | ModelApi::Responses
            ),
        },
        Kind::Image => image_api(provider).is_some(),
        Kind::Video => {
            provider.authentication == Authentication::ApiKey
                && matches!(
                    provider.api,
                    ModelApi::ChatCompletions | ModelApi::Responses
                )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::{Effort, ProviderId};

    fn fixture(api: ModelApi, authentication: Authentication, id: &str) -> (Provider, Model) {
        let model = Model {
            id: id.into(),
            display_name: None,
            context: 16000,
            output: 2048,
            vision: true,
            tools: true,
            reasoning: false,
            web_search: false,
            generates: vec![],
            efforts: vec![],
            capabilities: None,
            custom_efforts: false,
            default_effort: Effort::Default,
        };
        let provider = Provider {
            id: ProviderId::new(),
            revision: 1,
            name: "Media fixture".into(),
            api,
            authentication,
            endpoint: "https://example.test/v1".into(),
            options: None,
            oauth: None,
            enabled: true,
            models: vec![model.clone()],
            default_model: id.into(),
            credential: None,
        };
        (provider, model)
    }

    #[test]
    fn attempts_unknown_models_by_api() {
        for (api, route, video) in routes() {
            let (mut provider, mut model) = fixture(api, Authentication::ApiKey, "custom-model");
            provider.models.clear();
            model.tools = false;
            assert_eq!(image_api(&provider), route, "{api:?}");
            assert_eq!(image_route(&provider, &model), route, "{api:?}");
            assert_eq!(supports_api(Kind::Image, &provider), route.is_some());
            assert_eq!(supports_api(Kind::Video, &provider), video);
            assert!(supports_api(Kind::Vision, &provider));
            assert_eq!(supports(Kind::Image, &provider, &model), route.is_some());
            assert_eq!(supports(Kind::Video, &provider, &model), video);
            assert!(supports(Kind::Vision, &provider, &model));
        }
    }

    fn routes() -> [(ModelApi, Option<ImageRoute>, bool); 9] {
        [
            (ModelApi::ChatCompletions, Some(ImageRoute::Images), true),
            (ModelApi::Responses, Some(ImageRoute::Responses), true),
            (ModelApi::Anthropic, None, false),
            (ModelApi::Gemini, Some(ImageRoute::Native), false),
            (ModelApi::DeepSeek, None, false),
            (ModelApi::OpenCodeGo, None, false),
            (ModelApi::OpenCodeZen, None, false),
            (ModelApi::AzureOpenAi, None, false),
            (ModelApi::AzureAi, None, false),
        ]
    }

    #[test]
    fn selects_direct_output_from_metadata() {
        let (provider, mut model) =
            fixture(ModelApi::Responses, Authentication::ApiKey, "custom-model");
        assert_eq!(image_route(&provider, &model), Some(ImageRoute::Responses));
        model.generates = vec![Generation::Image];
        assert_eq!(image_route(&provider, &model), Some(ImageRoute::Images));
    }

    #[test]
    fn preserves_authentication_contracts() {
        for (api, authentication, image, vision) in [
            (
                ModelApi::Responses,
                Authentication::ChatGptOAuth,
                Some(ImageRoute::Authorized),
                true,
            ),
            (
                ModelApi::Responses,
                Authentication::Copilot,
                Some(ImageRoute::Authorized),
                true,
            ),
            (
                ModelApi::ChatCompletions,
                Authentication::Copilot,
                None,
                true,
            ),
            (ModelApi::Responses, Authentication::ChatGpt, None, true),
            (ModelApi::Gemini, Authentication::ChatGptOAuth, None, false),
        ] {
            let (provider, mut model) = fixture(api, authentication, "custom-model");
            model.generates = vec![Generation::Image, Generation::ImageTool, Generation::Video];
            assert_eq!(image_api(&provider), image);
            assert_eq!(image_route(&provider, &model), image);
            assert_eq!(supports_api(Kind::Vision, &provider), vision);
            assert_eq!(supports_api(Kind::Image, &provider), image.is_some());
            assert!(!supports_api(Kind::Video, &provider));
            assert_eq!(supports(Kind::Vision, &provider, &model), vision);
            assert!(!supports(Kind::Video, &provider, &model));
        }
    }

    #[test]
    fn respects_native_vision_denial() {
        for (api, _, _) in routes() {
            let (provider, mut model) = fixture(api, Authentication::ApiKey, "custom-model");
            model.capabilities = Some(sailry_protocol::conversation::discovery::Capabilities {
                vision: Some(false),
                ..Default::default()
            });
            model.vision = false;
            assert!(supports_api(Kind::Vision, &provider));
            assert!(!supports(Kind::Vision, &provider, &model));
        }
    }

    #[test]
    fn excludes_disabled_providers() {
        for (api, _, _) in routes() {
            let (mut provider, model) = fixture(api, Authentication::ApiKey, "custom-model");
            provider.enabled = false;
            assert_eq!(image_api(&provider), None);
            assert_eq!(image_route(&provider, &model), None);
            for kind in [Kind::Vision, Kind::Image, Kind::Video] {
                assert!(!supports_api(kind, &provider));
                assert!(!supports(kind, &provider, &model));
            }
        }
    }
}
