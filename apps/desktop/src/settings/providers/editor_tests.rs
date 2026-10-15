use super::*;
use core::prelude::v1::test;

#[test]
fn english_names() {
    let keys = super::super::data::Family::all()
        .map(|family| family.key())
        .chain(Preset::all().map(|preset| preset.key()));
    for key in keys {
        let label = rust_i18n::t!(key, locale = "en");
        assert!(label.is_ascii(), "provider name must remain English: {key}");
        assert_ne!(label.as_ref(), key, "missing provider name: {key}");
    }
    for locale in [
        "en", "zh-CN", "zh-TW", "ja", "ko", "fr", "de", "es", "pt-BR", "ru",
    ] {
        for preset in Preset::all() {
            let key = preset.key();
            assert_eq!(
                rust_i18n::t!(key, locale = locale),
                rust_i18n::t!(key, locale = "en"),
                "provider type name must remain English: {locale}: {key}"
            );
        }
    }
    for (preset, label) in [
        (Preset::ChatGptOAuth, "ChatGPT OAuth"),
        (Preset::ChatGpt, "ChatGPT SIWC"),
        (Preset::CompatibleResponses, "OpenAI Compatible"),
        (Preset::CompatibleAnthropic, "Anthropic Compatible"),
        (Preset::CompatibleGemini, "Gemini Compatible"),
    ] {
        assert_eq!(rust_i18n::t!(preset.key(), locale = "en").as_ref(), label);
    }
}

#[test]
fn channel_kinds() {
    let kinds = Preset::all()
        .filter(|preset| preset.kind() == *preset)
        .collect::<Vec<_>>();
    assert_eq!(
        kinds
            .iter()
            .map(|preset| tr(preset.key()))
            .collect::<HashSet<_>>()
            .len(),
        kinds.len()
    );
    for preset in Preset::all() {
        for variant in preset.protocols() {
            assert_eq!(variant.kind(), preset.kind());
            assert_eq!(variant.authentication(), preset.authentication());
        }
    }
    assert_eq!(Preset::CompatibleChat.kind(), Preset::CompatibleResponses);
    assert_eq!(Preset::CopilotResponses.kind(), Preset::Copilot);
    assert_ne!(
        Preset::ChatGptOAuth.authentication(),
        Preset::ChatGpt.authentication()
    );
}
