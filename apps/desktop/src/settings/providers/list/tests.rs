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
fn displays_current_account_label() {
    let mut value = channel(Preset::ChatGptOAuth);
    value.account_label = Some("account@example.test".into());
    assert_eq!(
        badge(&value, AuthKind::OAuth).as_ref(),
        "account@example.test"
    );
    assert_eq!(
        badge(&value, AuthKind::ApiKey),
        crate::tr(AuthKind::ApiKey.key())
    );
    value.account_label = None;
    assert_eq!(
        badge(&value, AuthKind::OAuth),
        crate::tr(AuthKind::OAuth.key())
    );
}
