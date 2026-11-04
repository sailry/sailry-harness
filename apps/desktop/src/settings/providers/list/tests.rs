use super::super::data::{Model, Preset, Store};
use super::summary::*;
use sailry_protocol::{
    Authentication,
    conversation::{presets::Source, quota::Window},
};

fn channel(preset: Preset) -> super::Channel {
    let mut channel = Store::default().channels.remove(0);
    channel.preset = preset;
    channel
}

fn packaged(mut channel: super::Channel) -> super::Channel {
    let mut entry = super::super::presets::bundled().remove(0);
    entry.source = Source::Package {
        name: "sample.models".into(),
    };
    channel.directory = Some(entry);
    channel
}

#[test]
fn authentication_kinds() {
    for (case, channel, authentication, expected) in [
        (
            "API key",
            channel(Preset::CompatibleChat),
            Authentication::ApiKey,
            AuthKind::ApiKey,
        ),
        (
            "OAuth",
            channel(Preset::ChatGptOAuth),
            Authentication::ChatGptOAuth,
            AuthKind::OAuth,
        ),
        (
            "packaged",
            packaged(channel(Preset::CompatibleChat)),
            Authentication::ApiKey,
            AuthKind::Plugin,
        ),
        (
            "declared plugin",
            channel(Preset::Plugin),
            Authentication::Plugin,
            AuthKind::Plugin,
        ),
    ] {
        assert_eq!(AuthKind::of(&channel, authentication), expected, "{case}");
    }
}

#[test]
fn credential_requirements() {
    let mut named = channel(Preset::CompatibleChat);
    named.credential_configured = false;
    named.directory = super::super::presets::bundled()
        .into_iter()
        .find(|entry| entry.preset.id == "qwen");
    assert_eq!(
        Status::of(&named, Authentication::ApiKey),
        Some(Status::MissingKey),
        "named route"
    );
    named.directory.as_mut().unwrap().preset.auth =
        sailry_protocol::conversation::presets::Auth::None;
    assert_eq!(
        Status::of(&named, Authentication::ApiKey),
        None,
        "keyless named route"
    );
    for (case, preset, configured, authentication, expected) in [
        (
            "configured hosted",
            Preset::OpenAi,
            true,
            Authentication::ApiKey,
            None,
        ),
        (
            "missing hosted",
            Preset::OpenAi,
            false,
            Authentication::ApiKey,
            Some(Status::MissingKey),
        ),
        (
            "missing cloud",
            Preset::AzureOpenAi,
            false,
            Authentication::ApiKey,
            Some(Status::MissingKey),
        ),
        (
            "keyless custom",
            Preset::CompatibleChat,
            false,
            Authentication::ApiKey,
            None,
        ),
        (
            "signed out",
            Preset::ChatGptOAuth,
            false,
            Authentication::ChatGptOAuth,
            Some(Status::SignedOut),
        ),
    ] {
        let mut channel = channel(preset);
        channel.credential_configured = configured;
        assert_eq!(Status::of(&channel, authentication), expected, "{case}");
    }
}

#[test]
fn model_chips() {
    for (case, ids, default, limit, expected, hidden) in [
        (
            "default first",
            vec!["a".into(), "b".into(), "c".into()],
            "c",
            8,
            vec!["c".to_owned(), "a".to_owned(), "b".to_owned()],
            0,
        ),
        (
            "missing default",
            vec!["a".into(), "b".into(), "c".into()],
            "missing",
            8,
            vec!["a".into(), "b".into(), "c".into()],
            0,
        ),
        (
            "hidden",
            (0..11).map(|index| format!("m{index}")).collect(),
            "m10",
            CHIPS,
            std::iter::once("m10".into())
                .chain((0..CHIPS - 1).map(|index| format!("m{index}")))
                .collect(),
            3,
        ),
    ] {
        let models: Vec<_> = ids.iter().map(|id: &String| Model::example(id)).collect();
        let (shown, count) = chips(&models, default, limit);
        assert_eq!(shown, expected, "{case}: shown");
        assert_eq!(count, hidden, "{case}: hidden");
    }
}

#[test]
fn matches_names_and_models() {
    let mut channel = channel(Preset::OpenAi);
    channel.name = "Work Buddy".into();
    channel.models = vec![Model::example("kimi-k2.6")];
    assert!(matches(&channel, ""));
    assert!(matches(&channel, " buddy "));
    assert!(matches(&channel, "KIMI"));
    assert!(!matches(&channel, "glm"));
}

#[test]
fn reports_live_remaining_only() {
    let window = Window {
        used_basis_points: 4_250,
        reset_at_ms: Some(100),
    };
    assert_eq!(remaining(&window, 99), Some(57.5));
    assert_eq!(remaining(&window, 100), None);
    let over = Window {
        used_basis_points: 10_050,
        reset_at_ms: None,
    };
    assert_eq!(remaining(&over, 0), Some(0.));
    for (used_basis_points, expected) in [(0, 100.), (10_000, 0.)] {
        assert_eq!(
            remaining(
                &Window {
                    used_basis_points,
                    reset_at_ms: None
                },
                0
            ),
            Some(expected)
        );
    }
}

#[test]
fn prefers_account_identity_over_type() {
    for preset in [Preset::ChatGptOAuth, Preset::Plugin, Preset::CompatibleChat] {
        let mut value = channel(preset);
        value.account_label = Some("account@example.test".into());
        assert_eq!(subtitle(&value).as_ref(), "account@example.test");
        for label in [None, Some(String::new()), Some("  ".into())] {
            value.account_label = label;
            assert_eq!(subtitle(&value), value.label());
        }
    }
}

mod lines {
    use super::*;
    use gpui_kit::AssetSource as _;
    use sailry_protocol::plugin::ui::Detail;
    use serde_json::json;

    fn details(value: serde_json::Value) -> Vec<Detail> {
        serde_json::from_value(value).unwrap()
    }

    fn labels(lines: &[Line]) -> Vec<Option<&str>> {
        lines.iter().map(|line| line.label.as_deref()).collect()
    }

    #[test]
    fn usage_lines() {
        for (case, rows, fallback, locale, expected) in [
            (
                "one meter",
                json!([{"label":{"label":"5h"},"value":"90%","progress":90}]),
                Some("64%"),
                "en",
                vec![Line {
                    value: "90%".into(),
                    label: Some("5h".into()),
                    progress: Some(90.),
                }],
            ),
            (
                "meter limit",
                json!([
                    {"label":{"label":"5h"},"value":"90%","progress":90},
                    {"label":{"label":"7d"},"value":"40%","progress":40},
                    {"label":{"label":"Month"},"value":"10%","progress":10}
                ]),
                None,
                "en",
                vec![
                    Line {
                        value: "90%".into(),
                        label: Some("5h".into()),
                        progress: Some(90.),
                    },
                    Line {
                        value: "40%".into(),
                        label: Some("7d".into()),
                        progress: Some(40.),
                    },
                ],
            ),
            (
                "meters before values",
                json!([
                    {"label":{"label":"Credits"},"value":"41205.37"},
                    {"label":{"label":"7d left"},"value":"91%","progress":91},
                    {"label":{"label":"Low"},"value":"0%","progress":-4}
                ]),
                Some("888.00"),
                "en",
                vec![
                    Line {
                        value: "91%".into(),
                        label: Some("7d left".into()),
                        progress: Some(91.),
                    },
                    Line {
                        value: "0%".into(),
                        label: Some("Low".into()),
                        progress: Some(0.),
                    },
                ],
            ),
            (
                "first value",
                json!([
                    {"label":{"label":"Credits"},"value":"2550.59"},
                    {"label":{"label":"Spent"},"value":"12"}
                ]),
                Some("888.00"),
                "en",
                vec![Line {
                    value: "2550.59".into(),
                    label: None,
                    progress: None,
                }],
            ),
            (
                "fallback",
                json!([]),
                Some("888.00"),
                "en",
                vec![Line {
                    value: "888.00".into(),
                    label: None,
                    progress: None,
                }],
            ),
            (
                "localized labels",
                json!([
                    {"label":{"label":"7d","locales":{"zh-CN":"7天"}},"value":"9%","progress":9},
                    {"label":{"label":"  "},"value":"3%","progress":3}
                ]),
                None,
                "zh-CN",
                vec![
                    Line {
                        value: "9%".into(),
                        label: Some("7天".into()),
                        progress: Some(9.),
                    },
                    Line {
                        value: "3%".into(),
                        label: None,
                        progress: Some(3.),
                    },
                ],
            ),
            ("empty", json!([]), None, "en", vec![]),
        ] {
            let actual = lines(&details(rows), fallback.map(Into::into), locale);
            assert_eq!(actual, expected, "{case}");
            if case == "meter limit" {
                assert_eq!(labels(&actual), [Some("5h"), Some("7d")], "{case}: labels");
                assert_eq!(actual.len(), METERS, "{case}: limit");
            }
        }
        let path = gpui_kit::assets::IconName::Coins.path();
        assert!(
            crate::assets::Assets.load(&path).unwrap().is_some(),
            "credits icon"
        );
    }
}
