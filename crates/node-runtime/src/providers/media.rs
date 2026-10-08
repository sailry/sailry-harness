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
    if !provider.enabled {
        return None;
    }
    if model.generates.contains(&Generation::Image)
        && matches!(
            (provider.api, provider.authentication),
            (ModelApi::Gemini | ModelApi::Vertex, Authentication::ApiKey)
                | (ModelApi::Vertex, Authentication::Host)
        )
    {
        return Some(ImageRoute::Native);
    }
    if provider.authentication == Authentication::ChatGptOAuth {
        return (provider.api == ModelApi::Responses
            && model.generates.contains(&Generation::ImageTool))
        .then_some(ImageRoute::Authorized);
    }
    if provider.authentication != Authentication::ApiKey {
        return None;
    }
    if matches!(
        provider.api,
        ModelApi::ChatCompletions | ModelApi::Responses
    ) && model.generates.contains(&Generation::Image)
    {
        return Some(ImageRoute::Images);
    }
    if provider.api == ModelApi::Responses
        && (model.generates.contains(&Generation::ImageTool) || hosted(&model.id))
    {
        return Some(ImageRoute::Responses);
    }
    None
}

pub(crate) fn supports(kind: Kind, provider: &Provider, model: &Model) -> bool {
    if !provider.enabled {
        return false;
    }
    match kind {
        Kind::Vision => {
            model.vision
                && match provider.authentication {
                    Authentication::ApiKey => true,
                    Authentication::Host => {
                        matches!(provider.api, ModelApi::Bedrock | ModelApi::Vertex)
                    }
                    Authentication::ChatGpt | Authentication::ChatGptOAuth => {
                        provider.api == ModelApi::Responses
                    }
                    Authentication::Copilot => matches!(
                        provider.api,
                        ModelApi::ChatCompletions | ModelApi::Responses
                    ),
                }
        }
        Kind::Image => image_route(provider, model).is_some(),
        Kind::Video => {
            provider.authentication == Authentication::ApiKey
                && matches!(
                    provider.api,
                    ModelApi::ChatCompletions | ModelApi::Responses
                )
                && model.generates.contains(&Generation::Video)
        }
    }
}

fn hosted(id: &str) -> bool {
    // Exact documented IDs, not generic function calling or a model-family prefix.
    // https://developers.openai.com/api/docs/guides/tools-image-generation
    // https://developers.openai.com/api/docs/models/gpt-6-astra
    [
        "gpt-6-astra",
        "gpt-5.5",
        "gpt-5.4-mini",
        "gpt-5.4-nano",
        "gpt-5.2",
        "gpt-5",
        "gpt-5-nano",
        "o3",
        "gpt-4.1",
        "gpt-4.1-mini",
        "gpt-4.1-nano",
        "gpt-4o",
        "gpt-4o-mini",
    ]
    .contains(&id)
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
    fn preserves_direct_companions() {
        for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
            let (provider, mut model) = fixture(api, Authentication::ApiKey, "image-model");
            model.generates.push(Generation::Image);
            assert_eq!(image_route(&provider, &model), Some(ImageRoute::Images));
            assert!(supports(Kind::Image, &provider, &model));
        }
    }

    #[test]
    fn requires_a_declared_or_documented_tool() {
        let (mut provider, mut model) =
            fixture(ModelApi::Responses, Authentication::ApiKey, "unknown-model");
        assert_eq!(image_route(&provider, &model), None);
        model.generates.push(Generation::ImageTool);
        assert_eq!(image_route(&provider, &model), Some(ImageRoute::Responses));
        provider.api = ModelApi::ChatCompletions;
        assert_eq!(image_route(&provider, &model), None);
        provider.api = ModelApi::Responses;
        provider.enabled = false;
        assert_eq!(image_route(&provider, &model), None);
        for id in ["gpt-5.5", "gpt-5.4-mini", "gpt-4.1", "gpt-6-astra"] {
            let (provider, model) = fixture(ModelApi::Responses, Authentication::ApiKey, id);
            assert_eq!(image_route(&provider, &model), Some(ImageRoute::Responses));
        }
        for id in [
            "gpt-6.1-sol",
            "gpt-5.5-unknown",
            "gpt-image-1",
            "unknown-model",
        ] {
            let (provider, model) = fixture(ModelApi::Responses, Authentication::ApiKey, id);
            assert_eq!(image_route(&provider, &model), None);
        }
    }

    #[test]
    fn excludes_login_hosted_generation() {
        for authentication in [
            Authentication::ChatGpt,
            Authentication::Copilot,
            Authentication::Host,
        ] {
            let (provider, mut model) = fixture(ModelApi::Responses, authentication, "gpt-5.5");
            model.generates = vec![Generation::Image, Generation::ImageTool, Generation::Video];
            assert_eq!(image_route(&provider, &model), None);
            assert!(!supports(Kind::Image, &provider, &model));
            assert!(!supports(Kind::Video, &provider, &model));
        }
    }

    #[test]
    fn retains_native_vision_authentication() {
        for (api, authentication) in [
            (ModelApi::Gemini, Authentication::ApiKey),
            (ModelApi::Responses, Authentication::ChatGpt),
            (ModelApi::Responses, Authentication::ChatGptOAuth),
            (ModelApi::ChatCompletions, Authentication::Copilot),
            (ModelApi::Vertex, Authentication::Host),
        ] {
            let (provider, model) = fixture(api, authentication, "vision-model");
            assert!(supports(Kind::Vision, &provider, &model));
        }
    }

    #[test]
    fn requires_declared_backend_generation() {
        let (mut provider, mut model) = fixture(
            ModelApi::Responses,
            Authentication::ChatGptOAuth,
            "gpt-6.1-sol",
        );
        assert_eq!(image_route(&provider, &model), None);
        model.generates.push(Generation::ImageTool);
        assert_eq!(image_route(&provider, &model), Some(ImageRoute::Authorized));
        assert!(supports(Kind::Image, &provider, &model));
        assert!(!supports(Kind::Video, &provider, &model));
        provider.api = ModelApi::ChatCompletions;
        assert_eq!(image_route(&provider, &model), None);
        provider.api = ModelApi::Responses;
        provider.authentication = Authentication::ChatGpt;
        assert_eq!(image_route(&provider, &model), None);
    }

    #[test]
    fn requires_declared_native_output() {
        for (api, authentication) in [
            (ModelApi::Gemini, Authentication::ApiKey),
            (ModelApi::Vertex, Authentication::ApiKey),
            (ModelApi::Vertex, Authentication::Host),
        ] {
            let (provider, mut model) = fixture(api, authentication, "native-image-model");
            assert_eq!(image_route(&provider, &model), None);
            model.generates.push(Generation::Image);
            assert_eq!(image_route(&provider, &model), Some(ImageRoute::Native));
            assert!(supports(Kind::Image, &provider, &model));
        }
    }
}
