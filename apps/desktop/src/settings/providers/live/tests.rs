use super::*;
use core::prelude::v1::test;

fn provider(api: ModelApi, endpoint: &str) -> Provider {
    Provider {
        request_max_retries: None,
        stream_max_retries: None,
        preset: None,
        binding: None,
        consent: None,
        id: ProviderId::new(),
        revision: 1,
        name: "Provider".into(),
        api,
        authentication: sailry_protocol::Authentication::ApiKey,
        endpoint: endpoint.into(),
        options: None,
        oauth: None,
        enabled: true,
        models: vec![],
        default_model: String::new(),
        credential: None,
    }
}

#[test]
fn logos_follow_configured_type() {
    for (api, endpoint, expected) in [
        (
            ModelApi::Responses,
            "https://api.openai.com/v1",
            Some("icons/providers/openai.svg"),
        ),
        (
            ModelApi::ChatCompletions,
            "https://api.x.ai/v1",
            Some("icons/providers/xai.svg"),
        ),
        (
            ModelApi::Anthropic,
            "https://proxy.example/v1",
            Some("icons/providers/anthropic.svg"),
        ),
        (
            ModelApi::AzureOpenAi,
            "https://example.openai.azure.com",
            None,
        ),
    ] {
        assert_eq!(logo(&provider(api, endpoint)).as_deref(), expected);
    }
}

#[test]
fn stable_preset_outweighs_endpoint_guessing() {
    let entry = super::super::presets::bundled()
        .into_iter()
        .find(|entry| entry.preset.id == "qwen")
        .unwrap();
    let mut provider = provider(ModelApi::ChatCompletions, "https://proxy.example/v1");
    provider.preset = Some(super::super::presets::reference(
        &entry,
        &entry.preset.routes[0],
    ));
    assert_eq!(
        preset(&provider),
        Preset::Hosted(super::super::vendors::Vendor::Qwen)
    );
    assert_eq!(logo(&provider), entry.preset.logo);
    provider.preset = None;
    assert_eq!(preset(&provider), Preset::CompatibleChat);
}

#[test]
fn distinguishes_configuration_and_access() {
    for (code, expected) in [
        (ErrorCode::PermissionDenied, "provider_access_denied"),
        (ErrorCode::NotConfigured, "provider_not_configured"),
        (ErrorCode::RevisionConflict, "provider_revision_changed"),
    ] {
        assert_eq!(
            error_key(&Fault::new(code, "Synthetic discovery failure")),
            expected
        );
    }
}

#[test]
fn rejects_stale_credential_metadata_reads() {
    let captured = View {
        connected: true,
        credential_cursor: 7,
        ..Default::default()
    };
    let mut current = captured.clone();
    assert!(metadata_current(&captured, &current));
    current.credential_cursor = 8;
    assert!(!metadata_current(&captured, &current));
    current.credential_cursor = 7;
    current.connected = false;
    assert!(!metadata_current(&captured, &current));
}
