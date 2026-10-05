//! UI language belongs to the controller, not the selected execution Node.
use gpui_kit::App;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Language {
    #[default]
    System,
    English,
    Chinese,
    TraditionalChinese,
    Japanese,
    Korean,
    French,
    German,
    Spanish,
    PortugueseBrazil,
    Russian,
}

impl Language {
    pub(crate) const ALL: [Self; 11] = [
        Self::System,
        Self::English,
        Self::Chinese,
        Self::TraditionalChinese,
        Self::Japanese,
        Self::Korean,
        Self::French,
        Self::German,
        Self::Spanish,
        Self::PortugueseBrazil,
        Self::Russian,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::System => "settings_language_system",
            Self::English => "settings_language_english",
            Self::Chinese => "settings_language_chinese",
            Self::TraditionalChinese => "settings_language_traditional_chinese",
            Self::Japanese => "settings_language_japanese",
            Self::Korean => "settings_language_korean",
            Self::French => "settings_language_french",
            Self::German => "settings_language_german",
            Self::Spanish => "settings_language_spanish",
            Self::PortugueseBrazil => "settings_language_portuguese_brazil",
            Self::Russian => "settings_language_russian",
        }
    }

    fn resolve(self, locales: impl IntoIterator<Item = String>) -> &'static str {
        match self {
            Self::English => "en",
            Self::Chinese => "zh-CN",
            Self::TraditionalChinese => "zh-TW",
            Self::Japanese => "ja",
            Self::Korean => "ko",
            Self::French => "fr",
            Self::German => "de",
            Self::Spanish => "es",
            Self::PortugueseBrazil => "pt-BR",
            Self::Russian => "ru",
            Self::System => locales
                .into_iter()
                .find_map(|locale| preferred(&locale))
                .unwrap_or("en"),
        }
    }
}

fn preferred(value: &str) -> Option<&'static str> {
    let normalized = value.replace('_', "-").to_ascii_lowercase();
    let parts: Vec<_> = normalized.split('.').next()?.split('-').collect();
    match *parts.first()? {
        "zh" => Some(
            if parts.contains(&"hant")
                || (!parts.contains(&"hans")
                    && parts.iter().any(|part| ["tw", "hk", "mo"].contains(part)))
            {
                "zh-TW"
            } else {
                "zh-CN"
            },
        ),
        "pt" if parts.contains(&"br") => Some("pt-BR"),
        "en" => Some("en"),
        "ja" => Some("ja"),
        "ko" => Some("ko"),
        "fr" => Some("fr"),
        "de" => Some("de"),
        "es" => Some("es"),
        "ru" => Some("ru"),
        _ => None,
    }
}

pub(crate) fn init(cx: &App) {
    let language = crate::preferences::data(cx).language.unwrap_or_default();
    let locale = language.resolve(sys_locale::get_locales());
    rust_i18n::set_locale(locale);
    gpui_kit::component::set_locale(locale);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_resources_resolve() {
        for line in include_str!("../../../locales/application.en.yml").lines() {
            let (key, value) = line.split_once(": ").unwrap();
            let value: String = serde_json::from_str(value).unwrap();
            assert_eq!(rust_i18n::t!(key, locale = "en"), value, "{key}");
        }
    }

    #[test]
    fn additional_resources_resolve() {
        let resources = [
            (
                "zh-TW",
                include_str!("../../../locales/application.zh-TW.yml"),
            ),
            ("ja", include_str!("../../../locales/application.ja.yml")),
            ("ko", include_str!("../../../locales/application.ko.yml")),
            ("fr", include_str!("../../../locales/application.fr.yml")),
            ("de", include_str!("../../../locales/application.de.yml")),
            ("es", include_str!("../../../locales/application.es.yml")),
            (
                "pt-BR",
                include_str!("../../../locales/application.pt-BR.yml"),
            ),
            ("ru", include_str!("../../../locales/application.ru.yml")),
        ];
        for (locale, resource) in resources {
            for line in resource.lines() {
                let (key, value) = line.split_once(": ").unwrap();
                let value: String = serde_json::from_str(value).unwrap();
                assert_eq!(
                    rust_i18n::t!(key, locale = locale),
                    value,
                    "{locale}: {key}"
                );
            }
        }
    }

    #[gpui_kit::test]
    fn applies_saved_language_on_start(cx: &mut gpui_kit::TestAppContext) {
        let before = rust_i18n::locale().to_string();
        let kit_before = gpui_kit::component::locale().to_string();
        for language in Language::ALL
            .into_iter()
            .filter(|value| *value != Language::System)
        {
            let expected = language.resolve([]);
            cx.update(|cx| {
                gpui_kit::init(cx);
                crate::preferences::update(cx, |data| data.language = Some(language));
                init(cx);
                assert_eq!(rust_i18n::locale().to_string(), expected);
                assert_eq!(gpui_kit::component::locale().to_string(), expected);
            });
        }
        rust_i18n::set_locale(&before);
        gpui_kit::component::set_locale(&kit_before);
    }

    #[test]
    fn follows_supported_system_preferences() {
        let cases: &[(&[&str], &str)] = &[
            (&["zh-Hans-CN"], "zh-CN"),
            (&["zh_CN.UTF-8"], "zh-CN"),
            (&["zh-Hans-TW"], "zh-CN"),
            (&["zh-TW"], "zh-TW"),
            (&["zh_HK.UTF-8"], "zh-TW"),
            (&["zh-MO"], "zh-TW"),
            (&["zh-Hant"], "zh-TW"),
            (&["fr-CA", "zh-TW"], "fr"),
            (&["de-DE"], "de"),
            (&["es-MX"], "es"),
            (&["ja-JP"], "ja"),
            (&["ko-KR"], "ko"),
            (&["pt_BR.UTF-8"], "pt-BR"),
            (&["ru-RU"], "ru"),
            (&["ar-SA", "es-ES"], "es"),
            (&["pt-PT"], "en"),
        ];
        for (locales, expected) in cases {
            assert_eq!(
                Language::System.resolve(locales.iter().map(|value| value.to_string())),
                *expected
            );
        }
        assert_eq!(
            Language::System.resolve(["en-US".into(), "zh-CN".into()]),
            "en"
        );
        assert_eq!(Language::System.resolve(["ar-SA".into()]), "en");
        assert_eq!(Language::System.resolve([]), "en");
    }

    #[test]
    fn explicit_choices_override_system() {
        assert_eq!(Language::English.resolve(["zh-CN".into()]), "en");
        assert_eq!(Language::Chinese.resolve(["en-US".into()]), "zh-CN");
        let choices = [
            (Language::TraditionalChinese, "zh-TW"),
            (Language::Japanese, "ja"),
            (Language::Korean, "ko"),
            (Language::French, "fr"),
            (Language::German, "de"),
            (Language::Spanish, "es"),
            (Language::PortugueseBrazil, "pt-BR"),
            (Language::Russian, "ru"),
        ];
        assert_eq!(Language::ALL.len(), 11);
        for (language, locale) in choices {
            assert_eq!(language.resolve(["en-US".into()]), locale);
            assert_eq!(
                serde_json::from_str::<Language>(&serde_json::to_string(&language).unwrap())
                    .unwrap(),
                language
            );
            assert_ne!(
                rust_i18n::t!(language.label(), locale = "en"),
                language.label()
            );
        }
        assert_eq!(
            crate::preferences::Data::default()
                .language
                .unwrap_or_default(),
            Language::System
        );
    }
}
