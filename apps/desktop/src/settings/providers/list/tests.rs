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

mod auth {
    use super::*;

    #[test]
    fn follows_authentication() {
        let key = channel(Preset::CompatibleChat);
        assert_eq!(AuthKind::of(&key, Authentication::ApiKey), AuthKind::ApiKey);
        let login = channel(Preset::ChatGptOAuth);
        assert_eq!(
            AuthKind::of(&login, Authentication::ChatGptOAuth),
            AuthKind::OAuth
        );
    }

    #[test]
    fn marks_plugin_channels() {
        let channel = packaged(channel(Preset::CompatibleChat));
        assert_eq!(
            AuthKind::of(&channel, Authentication::ApiKey),
            AuthKind::Plugin
        );
        let declared = super::channel(Preset::Plugin);
        assert_eq!(
            AuthKind::of(&declared, Authentication::Plugin),
            AuthKind::Plugin
        );
    }
}

mod status {
    use super::*;

    #[test]
    fn requires_hosted_keys() {
        let mut hosted = channel(Preset::OpenAi);
        assert_eq!(Status::of(&hosted, Authentication::ApiKey), None);
        hosted.credential_configured = false;
        assert_eq!(
            Status::of(&hosted, Authentication::ApiKey),
            Some(Status::MissingKey)
        );
        let mut cloud = channel(Preset::AzureOpenAi);
        cloud.credential_configured = false;
        assert_eq!(
            Status::of(&cloud, Authentication::ApiKey),
            Some(Status::MissingKey)
        );
    }

    #[test]
    fn allows_keyless_custom_endpoints() {
        let mut custom = channel(Preset::CompatibleChat);
        custom.credential_configured = false;
        assert_eq!(Status::of(&custom, Authentication::ApiKey), None);
    }

    #[test]
    fn named_routes_require_declared_keys() {
        let mut hosted = channel(Preset::CompatibleChat);
        hosted.credential_configured = false;
        hosted.directory = super::super::super::presets::bundled()
            .into_iter()
            .find(|entry| entry.preset.id == "qwen");
        assert_eq!(
            Status::of(&hosted, Authentication::ApiKey),
            Some(Status::MissingKey)
        );
        hosted.directory.as_mut().unwrap().preset.auth =
            sailry_protocol::conversation::presets::Auth::None;
        assert_eq!(Status::of(&hosted, Authentication::ApiKey), None);
    }

    #[test]
    fn reports_missing_login() {
        let mut login = channel(Preset::ChatGptOAuth);
        login.credential_configured = false;
        assert_eq!(
            Status::of(&login, Authentication::ChatGptOAuth),
            Some(Status::SignedOut)
        );
    }
}

mod chips {
    use super::*;

    fn models(ids: &[&str]) -> Vec<Model> {
        ids.iter().map(|id| Model::example(id)).collect()
    }

    #[test]
    fn leads_with_default() {
        let models = models(&["a", "b", "c"]);
        assert_eq!(chips(&models, "c", 8), (vec!["c", "a", "b"], 0));
        assert_eq!(chips(&models, "missing", 8), (vec!["a", "b", "c"], 0));
    }

    #[test]
    fn counts_hidden_models() {
        let ids = (0..11).map(|index| format!("m{index}")).collect::<Vec<_>>();
        let models = models(&ids.iter().map(String::as_str).collect::<Vec<_>>());
        let (shown, hidden) = chips(&models, "m10", CHIPS);
        assert_eq!(shown.len(), CHIPS);
        assert_eq!(shown[0], "m10");
        assert_eq!(hidden, 3);
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
    fn one_meter_one_line() {
        let rows = details(json!([{"label":{"label":"5h"},"value":"90%","progress":90}]));
        let lines = lines(&rows, Some("64%".into()), "en");
        assert_eq!(
            lines,
            [Line {
                value: "90%".into(),
                label: Some("5h".into()),
                progress: Some(90.),
            }]
        );
    }

    #[test]
    fn caps_meters_at_two() {
        let rows = details(json!([
            {"label":{"label":"5h"},"value":"90%","progress":90},
            {"label":{"label":"7d"},"value":"40%","progress":40},
            {"label":{"label":"Month"},"value":"10%","progress":10}
        ]));
        let lines = lines(&rows, None, "en");
        assert_eq!(labels(&lines), [Some("5h"), Some("7d")]);
        assert_eq!(lines.len(), METERS);
    }

    #[test]
    fn meters_take_precedence() {
        let rows = details(json!([
            {"label":{"label":"Credits"},"value":"41205.37"},
            {"label":{"label":"7d left"},"value":"91%","progress":91},
            {"label":{"label":"Low"},"value":"0%","progress":-4}
        ]));
        let lines = lines(&rows, Some("888.00".into()), "en");
        let values: Vec<_> = lines.iter().map(|line| line.value.as_ref()).collect();
        assert_eq!(values, ["91%", "0%"]);
        assert_eq!(lines[1].progress, Some(0.));
    }

    #[test]
    fn values_show_first_value() {
        let rows = details(json!([
            {"label":{"label":"Credits"},"value":"2550.59"},
            {"label":{"label":"Spent"},"value":"12"}
        ]));
        let expected = [Line {
            value: "2550.59".into(),
            label: None,
            progress: None,
        }];
        assert_eq!(lines(&rows, Some("888.00".into()), "en"), expected);
        let fallback = lines(&[], Some("888.00".into()), "en");
        assert_eq!(fallback[0].value.as_ref(), "888.00");
        assert_eq!(fallback[0].label, None);
    }

    #[test]
    fn localizes_and_omits_blank_labels() {
        let rows = details(json!([
            {"label":{"label":"7d","locales":{"zh-CN":"7天"}},"value":"9%","progress":9},
            {"label":{"label":"  "},"value":"3%","progress":3}
        ]));
        assert_eq!(labels(&lines(&rows, None, "zh-CN")), [Some("7天"), None]);
    }

    #[test]
    fn empty_without_usage() {
        assert!(lines(&[], None, "en").is_empty());
    }

    #[test]
    fn credits_icon_is_bundled() {
        let path = gpui_kit::assets::IconName::Coins.path();
        assert!(crate::assets::Assets.load(&path).unwrap().is_some());
    }
}
