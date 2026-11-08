use super::*;
use core::prelude::v1::test;

#[test]
fn declared_labels_and_kinds() {
    for (scenario, check) in [
        (
            "english_names",
            (|_scenario: &str| {
                let keys = super::super::data::Category::ALL
                    .into_iter()
                    .map(|category| category.key())
                    .chain(Preset::all().map(|preset| preset.key()));
                for key in keys {
                    let label = rust_i18n::t!(key, locale = "en");
                    assert!(
                        label.is_ascii(),
                        "english names: provider name must remain English: {key}"
                    );
                    assert_ne!(
                        label.as_ref(),
                        key,
                        "english names: missing provider name: {key}"
                    );
                }
                for locale in [
                    "en", "zh-CN", "zh-TW", "ja", "ko", "fr", "de", "es", "pt-BR", "ru",
                ] {
                    for preset in Preset::all() {
                        let key = preset.key();
                        assert_eq!(
                            rust_i18n::t!(key, locale = locale),
                            rust_i18n::t!(key, locale = "en"),
                            "english names: provider type name must remain English: {locale}: {key}"
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
                    assert_eq!(
                        rust_i18n::t!(preset.key(), locale = "en").as_ref(),
                        label,
                        "english names"
                    );
                }
            }) as fn(&str),
        ),
        (
            "channel_kinds",
            (|_scenario: &str| {
                let kinds = Preset::all()
                    .filter(|preset| preset.kind() == *preset)
                    .collect::<Vec<_>>();
                assert_eq!(
                    kinds
                        .iter()
                        .map(|preset| tr(preset.key()))
                        .collect::<HashSet<_>>()
                        .len(),
                    kinds.len(),
                    "channel kinds"
                );
                for preset in Preset::all() {
                    for variant in preset.protocols() {
                        assert_eq!(variant.kind(), preset.kind(), "channel kinds");
                        assert_eq!(
                            variant.authentication(),
                            preset.authentication(),
                            "channel kinds"
                        );
                    }
                }
                assert_eq!(
                    Preset::CompatibleChat.kind(),
                    Preset::CompatibleResponses,
                    "channel kinds"
                );
                assert_eq!(
                    Preset::CopilotResponses.kind(),
                    Preset::Copilot,
                    "channel kinds"
                );
                assert_ne!(
                    Preset::ChatGptOAuth.authentication(),
                    Preset::ChatGpt.authentication(),
                    "channel kinds"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
