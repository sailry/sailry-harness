use super::*;
use crate::settings::providers::vendors::Vendor;
use sailry_protocol::{
    Authentication, ProviderId,
    conversation::{Provider, cloud::Options},
};

#[gpui::test]
fn maps_preset_payloads(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let cases = [
        (
            Preset::OpenAi,
            ModelApi::Responses,
            "https://api.openai.com/v1",
        ),
        (
            Preset::CompatibleResponses,
            ModelApi::Responses,
            "https://fixture.invalid/responses",
        ),
        (
            Preset::CompatibleChat,
            ModelApi::ChatCompletions,
            "https://fixture.invalid/chat",
        ),
        (
            Preset::ChatGptOAuth,
            ModelApi::Responses,
            "https://chatgpt.com/backend-api/codex",
        ),
        (
            Preset::ChatGpt,
            ModelApi::Responses,
            "https://api.openai.com/v1",
        ),
        (
            Preset::Copilot,
            ModelApi::ChatCompletions,
            "https://api.githubcopilot.com",
        ),
        (
            Preset::CopilotResponses,
            ModelApi::Responses,
            "https://api.githubcopilot.com",
        ),
        (
            Preset::Anthropic,
            ModelApi::Anthropic,
            "https://api.anthropic.com",
        ),
        (
            Preset::CompatibleAnthropic,
            ModelApi::Anthropic,
            "https://fixture.invalid/anthropic",
        ),
        (
            Preset::Gemini,
            ModelApi::Gemini,
            "https://generativelanguage.googleapis.com/v1beta",
        ),
        (
            Preset::OpenCodeGo,
            ModelApi::OpenCodeGo,
            "https://opencode.ai/zen/go/v1",
        ),
        (
            Preset::OpenCodeZen,
            ModelApi::OpenCodeZen,
            "https://opencode.ai/zen/v1",
        ),
        (
            Preset::CompatibleGemini,
            ModelApi::Gemini,
            "https://fixture.invalid/gemini/v1beta",
        ),
        (
            Preset::AzureOpenAi,
            ModelApi::AzureOpenAi,
            "https://fixture.openai.azure.com",
        ),
        (
            Preset::AzureAi,
            ModelApi::AzureAi,
            "https://fixture.services.ai.azure.com/models",
        ),
        (
            Preset::Hosted(Vendor::Qwen),
            ModelApi::ChatCompletions,
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
        ),
        (
            Preset::Hosted(Vendor::Moonshot),
            ModelApi::ChatCompletions,
            "https://api.moonshot.ai/v1",
        ),
        (
            Preset::Hosted(Vendor::Xai),
            ModelApi::ChatCompletions,
            "https://api.x.ai/v1",
        ),
        (
            Preset::Hosted(Vendor::DeepSeek),
            ModelApi::DeepSeek,
            "https://api.deepseek.com",
        ),
        (
            Preset::Hosted(Vendor::Groq),
            ModelApi::ChatCompletions,
            "https://api.groq.com/openai/v1",
        ),
        (
            Preset::Hosted(Vendor::OpenRouter),
            ModelApi::ChatCompletions,
            "https://openrouter.ai/api/v1",
        ),
        (
            Preset::Hosted(Vendor::Fireworks),
            ModelApi::ChatCompletions,
            "https://api.fireworks.ai/inference/v1",
        ),
        (
            Preset::Hosted(Vendor::Together),
            ModelApi::ChatCompletions,
            "https://api.together.xyz/v1",
        ),
        (
            Preset::Hosted(Vendor::Mistral),
            ModelApi::ChatCompletions,
            "https://api.mistral.ai/v1",
        ),
        (
            Preset::Hosted(Vendor::Perplexity),
            ModelApi::ChatCompletions,
            "https://api.perplexity.ai",
        ),
        (
            Preset::Hosted(Vendor::Cerebras),
            ModelApi::ChatCompletions,
            "https://api.cerebras.ai/v1",
        ),
        (
            Preset::Hosted(Vendor::SambaNova),
            ModelApi::ChatCompletions,
            "https://api.sambanova.ai/v1",
        ),
        (
            Preset::Hosted(Vendor::MiniMax),
            ModelApi::ChatCompletions,
            "https://api.minimax.chat/v1",
        ),
        (
            Preset::Hosted(Vendor::Doubao),
            ModelApi::ChatCompletions,
            "https://ark.cn-beijing.volces.com/api/v3",
        ),
        (
            Preset::Hosted(Vendor::Zhipu),
            ModelApi::ChatCompletions,
            "https://open.bigmodel.cn/api/paas/v4",
        ),
        (
            Preset::Hosted(Vendor::Baidu),
            ModelApi::ChatCompletions,
            "https://qianfan.baidubce.com/v2",
        ),
        (
            Preset::Hosted(Vendor::Cohere),
            ModelApi::ChatCompletions,
            "https://api.cohere.com/compatibility/v1",
        ),
    ];
    assert_eq!(
        cases.map(|(preset, _, _)| preset).as_slice(),
        Preset::all().collect::<Vec<_>>()
    );
    let (_, visual) = cx.add_window_view(|window, cx| {
        let owner = cx.new(|cx| Workspace::new(window, cx));
        Root::new(owner, window, cx)
    });
    visual.update(|window, cx| {
        let owner = cx.new(|cx| Workspace::new(window, cx));
        for (preset, api, endpoint) in cases {
            let editor = cx.new(|cx| Editor::new(owner.clone(), None, window, cx));
            editor.update(cx, |editor, cx| {
                editor.select_preset(preset, window, cx);
                editor.name.update(cx, |input, cx| {
                    input.set_value("Fixture provider", window, cx)
                });
                let authentication = match preset {
                    Preset::ChatGptOAuth => Authentication::ChatGptOAuth,
                    Preset::ChatGpt => Authentication::ChatGpt,
                    Preset::Copilot | Preset::CopilotResponses => Authentication::Copilot,
                    _ => Authentication::ApiKey,
                };
                let custom_endpoint = matches!(
                    preset,
                    Preset::CompatibleResponses
                        | Preset::CompatibleChat
                        | Preset::CompatibleAnthropic
                        | Preset::CompatibleGemini
                        | Preset::AzureOpenAi
                        | Preset::AzureAi
                );
                assert_eq!(preset.custom_endpoint(), custom_endpoint);
                assert_eq!(preset.authentication(), authentication);
                assert_eq!(preset.key_auth(), authentication == Authentication::ApiKey);
                assert_eq!(
                    preset.oauth(),
                    matches!(
                        authentication,
                        Authentication::ChatGptOAuth
                            | Authentication::ChatGpt
                            | Authentication::Copilot
                    )
                );
                if custom_endpoint {
                    editor
                        .endpoint
                        .update(cx, |input, cx| input.set_value(endpoint, window, cx));
                }
                if authentication == Authentication::ApiKey {
                    editor.credential.update(cx, |input, cx| {
                        input.set_value("synthetic-editor-secret", window, cx)
                    });
                }
                editor.add_model(window, cx);
                editor.models[0]
                    .id
                    .update(cx, |input, cx| input.set_value("fixture-model", window, cx));
                let connection = editor.connection(cx).unwrap();
                assert_eq!(connection, (api, endpoint.into()));
                let channel = editor.channel(cx).unwrap();
                assert_eq!(channel.preset, preset);
                assert_eq!(channel.name, "Fixture provider");
                assert_eq!(channel.models.len(), 1);
                assert_eq!(channel.models[0].id, "fixture-model");
                assert_eq!(channel.default_model, "fixture-model");
                assert_eq!(
                    channel.credential_configured,
                    authentication == Authentication::ApiKey
                );
                assert_eq!(
                    channel.endpoint,
                    if custom_endpoint { endpoint } else { "" }
                );
                let options = match preset {
                    Preset::AzureOpenAi => Some(Options::AzureOpenAi {
                        api_version: "2024-10-21".into(),
                    }),
                    _ => None,
                };
                assert_eq!(channel.options, options);
                let provider = Provider {
                    id: ProviderId::new(),
                    revision: 1,
                    name: channel.name.clone(),
                    api: connection.0,
                    authentication: preset.authentication(),
                    endpoint: connection.1,
                    options: channel.options.clone(),
                    oauth: channel.oauth.clone(),
                    enabled: channel.enabled,
                    models: channel
                        .models
                        .iter()
                        .map(crate::settings::providers::live::model)
                        .collect(),
                    default_model: channel.default_model.clone(),
                    credential: None,
                };
                let restored = crate::settings::providers::live::channel(0, &provider);
                assert_eq!(restored.preset, preset);
                assert_eq!(restored.endpoint, endpoint);
                assert_eq!(restored.options, options);
                assert_eq!(restored.oauth, channel.oauth);
                assert_eq!(restored.name, channel.name);
                assert_eq!(restored.default_model, channel.default_model);
                assert_eq!(restored.models[0].id, "fixture-model");
            });
        }
    });
}
