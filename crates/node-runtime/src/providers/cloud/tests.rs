use super::*;
use sailry_protocol::{Authentication, ProviderId, conversation::connection::Product};

const APIS: [ModelApi; 9] = [
    ModelApi::ChatCompletions,
    ModelApi::Responses,
    ModelApi::Anthropic,
    ModelApi::Gemini,
    ModelApi::DeepSeek,
    ModelApi::OpenCodeGo,
    ModelApi::OpenCodeZen,
    ModelApi::AzureOpenAi,
    ModelApi::AzureAi,
];

fn provider(api: ModelApi, options: Option<Options>) -> Provider {
    Provider {
        request_max_retries: None,
        stream_max_retries: None,
        preset: None,
        binding: None,
        consent: None,
        id: ProviderId::new(),
        revision: 1,
        name: "Connection fixture".into(),
        api,
        authentication: Authentication::ApiKey,
        endpoint: "https://example.test/custom".into(),
        options,
        oauth: None,
        enabled: true,
        models: vec![],
        default_model: String::new(),
        credential: None,
    }
}

#[test]
fn accepts_hosted_only_for_supported_apis() {
    for api in APIS {
        for product in [Product::Metered, Product::Coding] {
            let provider = provider(api, Some(Options::Hosted { product }));
            assert_eq!(
                validate(&provider).is_ok(),
                matches!(
                    api,
                    ModelApi::ChatCompletions | ModelApi::Anthropic | ModelApi::DeepSeek
                ),
                "{api:?}, {product:?}"
            );
        }
    }
}

#[test]
fn preserves_unset_options() {
    for api in APIS {
        let provider = provider(api, None);
        assert_eq!(
            validate(&provider).is_ok(),
            api != ModelApi::AzureOpenAi,
            "{api:?}"
        );
        assert!(provider.options.is_none());
    }
}

#[test]
fn keeps_azure_version_validation() {
    for api in APIS {
        let provider = provider(
            api,
            Some(Options::AzureOpenAi {
                api_version: "2024-10-21".into(),
            }),
        );
        assert_eq!(
            validate(&provider).is_ok(),
            api == ModelApi::AzureOpenAi,
            "{api:?}"
        );
    }
    for api_version in ["", ".", "..", "2024/10/21", "2024?key=value"] {
        assert!(
            validate(&provider(
                ModelApi::AzureOpenAi,
                Some(Options::AzureOpenAi {
                    api_version: api_version.into(),
                })
            ))
            .is_err()
        );
    }
}
