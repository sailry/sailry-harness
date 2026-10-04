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
}

impl Language {
    pub(crate) const ALL: [Self; 3] = [Self::System, Self::English, Self::Chinese];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::System => "settings_language_system",
            Self::English => "settings_language_english",
            Self::Chinese => "settings_language_chinese",
        }
    }

    fn resolve(self, locales: impl IntoIterator<Item = String>) -> &'static str {
        match self {
            Self::English => "en",
            Self::Chinese => "zh-CN",
            Self::System => locales
                .into_iter()
                .find_map(|locale| {
                    match locale
                        .split(['-', '_'])
                        .next()?
                        .to_ascii_lowercase()
                        .as_str()
                    {
                        "zh" => Some("zh-CN"),
                        "en" => Some("en"),
                        _ => None,
                    }
                })
                .unwrap_or("en"),
        }
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

    #[gpui_kit::test]
    fn applies_saved_language_on_start(cx: &mut gpui_kit::TestAppContext) {
        let before = rust_i18n::locale().to_string();
        let kit_before = gpui_kit::component::locale().to_string();
        for (language, expected) in [(Language::English, "en"), (Language::Chinese, "zh-CN")] {
            cx.update(|cx| {
                gpui_kit::init(cx);
                crate::preferences::update(cx, |data| data.language = Some(language));
                init(cx);
                assert_eq!(rust_i18n::locale().to_string(), expected);
            });
        }
        rust_i18n::set_locale(&before);
        gpui_kit::component::set_locale(&kit_before);
    }

    #[test]
    fn follows_supported_system_preferences() {
        for locales in [
            vec!["zh-Hans-CN"],
            vec!["zh_CN.UTF-8"],
            vec!["fr-FR", "zh-TW"],
        ] {
            assert_eq!(
                Language::System.resolve(locales.into_iter().map(String::from)),
                "zh-CN"
            );
        }
        assert_eq!(
            Language::System.resolve(["en-US".into(), "zh-CN".into()]),
            "en"
        );
        assert_eq!(Language::System.resolve(["fr-FR".into()]), "en");
        assert_eq!(Language::System.resolve([]), "en");
    }

    #[test]
    fn explicit_choices_override_system() {
        assert_eq!(Language::English.resolve(["zh-CN".into()]), "en");
        assert_eq!(Language::Chinese.resolve(["en-US".into()]), "zh-CN");
        assert_eq!(
            crate::preferences::Data::default()
                .language
                .unwrap_or_default(),
            Language::System
        );
    }
}
