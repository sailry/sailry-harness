use super::*;

mod composer {
    use super::*;
    use sailry_protocol::{Effort, NodeId, ProviderId};

    fn selection() -> Composer {
        Composer {
            model: Some(Model {
                source: NodeId([1; 32]),
                provider: ProviderId::new(),
                model: "declared-model".into(),
                effort: Effort::High,
                features: Default::default(),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn missing_features_preserves_existing_selection() {
        let expected = selection();
        let mut value = serde_json::to_value(&expected).unwrap();
        value["model"].as_object_mut().unwrap().remove("features");
        let restored: Composer = serde_json::from_value(value).unwrap();
        assert_eq!(restored, expected);
        assert!(restored.model.unwrap().features.is_empty());
    }

    #[test]
    fn persists_features_independently_of_effort() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preferences.json");
        let mut preferences = Preferences::open(path.clone());
        preferences.data.composer = Some(selection());
        for value in [Some("fast"), Some("future-choice"), None] {
            let features = value
                .map(|value| ("speed".to_owned(), value.to_owned()))
                .into_iter()
                .collect::<std::collections::BTreeMap<_, _>>();
            preferences
                .data
                .composer
                .as_mut()
                .unwrap()
                .model
                .as_mut()
                .unwrap()
                .features = features.clone();
            preferences.save();
            assert_eq!(preferences.error, None);
            let restored = Preferences::open(path.clone());
            assert_eq!(restored.data.composer, preferences.data.composer);
            let model = restored.data.composer.unwrap().model.unwrap();
            assert_eq!(model.features, features);
            assert_eq!(model.effort, Effort::High);
        }
    }
}

mod picker {
    use super::*;
    #[test]
    fn missing_uses_list_without_hidden() {
        let mut value = serde_json::to_value(Data::default()).unwrap();
        value.as_object_mut().unwrap().remove("file_picker");
        let data: Data = serde_json::from_value(value).unwrap();
        assert_eq!(
            data.file_picker.unwrap_or_default(),
            FilePicker {
                grid: false,
                hidden: false
            }
        );
        assert_eq!(data.version, 1);
    }
    #[test]
    fn round_trip_preserves_other_choices() {
        let mut data = Data {
            toast_seconds: 8,
            ..Default::default()
        };
        data.notifications[0] = false;
        data.file_picker = Some(FilePicker {
            grid: true,
            hidden: true,
        });
        let restored: Data = serde_json::from_slice(&serde_json::to_vec(&data).unwrap()).unwrap();
        assert_eq!(restored, data);
        assert_eq!(restored.version, 1);
    }
}

mod message_size {
    use super::*;

    #[test]
    fn defaults_to_standard() {
        assert_eq!(Data::default().message_text_size(), 14);
        assert_eq!(Data::MESSAGE_TEXT_SIZES, [12, 14, 16, 18]);
    }

    #[test]
    fn persists_each_step() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preferences.json");
        for size in Data::MESSAGE_TEXT_SIZES {
            let mut preferences = Preferences::open(path.clone());
            preferences.data.message_text_size = Some(size);
            preferences.save();
            assert_eq!(preferences.error, None);
            let restored = Preferences::open(path.clone());
            assert_eq!(restored.data.message_text_size(), size);
            assert_eq!(restored.data.version, 1);
            assert_eq!(restored.data.terminal, Terminal::default());
        }
    }

    #[test]
    fn rejects_unsupported_without_overwrite() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preferences.json");
        let data = Data {
            message_text_size: Some(15),
            ..Default::default()
        };
        let bytes = serde_json::to_vec(&data).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let mut preferences = Preferences::open(path.clone());
        assert_eq!(preferences.error, Some("preferences_read_failed"));
        preferences.save();
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}

mod update_channel {
    use super::*;
    use crate::updater::Channel;

    #[test]
    fn defaults_from_installed_version() {
        assert_eq!(Channel::initial("0.1.0-alpha.1"), Channel::Experience);
        assert_eq!(Channel::initial("0.1.0"), Channel::Stable);
        assert_eq!(Channel::initial("0.1.0+build.2"), Channel::Stable);
        assert_eq!(Data::default().update_channel, None);
    }

    #[test]
    fn preserves_explicit_choice_across_versions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preferences.json");
        let mut preferences = Preferences::open(path.clone());
        preferences.data.update_channel = Some(Channel::Stable);
        preferences.save();
        assert_eq!(preferences.error, None);
        assert_eq!(
            Preferences::open(path).data.update_channel,
            Some(Channel::Stable)
        );
    }

    #[test]
    fn alpha_choice_survives_stable_reload() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preferences.json");
        let mut preferences = Preferences::open(path.clone());
        let initial = Channel::initial("0.1.0-alpha.1");
        preferences.data.update_channel = Some(initial);
        preferences.save();

        let restored = Preferences::open(path);
        assert_eq!(
            restored
                .data
                .update_channel
                .unwrap_or_else(|| Channel::initial("0.1.0")),
            Channel::Experience
        );
    }
}

#[test]
fn sidebar_metrics_are_opt_in_and_persisted() {
    assert!(!Data::default().sidebar_metrics);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    let mut preferences = Preferences::open(path.clone());
    assert!(!preferences.data.sidebar_metrics);
    preferences.data.sidebar_metrics = true;
    preferences.save();
    assert_eq!(preferences.error, None);
    assert!(Preferences::open(path).data.sidebar_metrics);
}

#[test]
fn preserves_failed_changes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    let mut preferences = Preferences::open(path.clone());
    assert!(!path.exists());
    preferences.data.notifications[2] = false;
    preferences.data.toast_seconds = 8;
    preferences.data.surfaces = Some(Surfaces {
        main: 45.,
        sidebar: Some(73.),
    });
    preferences.data.browser_persistent = Some(false);
    preferences.data.message_display = Some(MessageDisplay::Compact);
    preferences.data.dictation = Some(Dictation {
        microphone: Some("fixture-device-id".into()),
        language: Language::English,
        model: None,
    });
    preferences.data.pairing = "https://pairing.example.test".into();
    preferences.data.iroh_relays = Some(vec!["https://relay.example.test".into()]);
    std::fs::create_dir(&path).unwrap();
    preferences.save();
    assert_eq!(preferences.error, Some("preferences_save_failed"));
    assert_eq!(preferences.data.toast_seconds, 8);
    std::fs::remove_dir(&path).unwrap();
    preferences.save();
    assert_eq!(preferences.error, None);
    assert_eq!(Preferences::open(path.clone()).data, preferences.data);
    std::fs::write(&path, b"invalid preference data").unwrap();
    let mut unavailable = Preferences::open(path.clone());
    unavailable.data.toast_seconds = 3;
    unavailable.save();
    assert_eq!(unavailable.error, Some("preferences_read_failed"));
    assert_eq!(std::fs::read(path).unwrap(), b"invalid preference data");
}

#[test]
fn rejects_invalid_relays_without_overwriting() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    let data = Data {
        iroh_relays: Some(vec!["http://relay.example.test".into()]),
        ..Default::default()
    };
    let bytes = serde_json::to_vec(&data).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let mut preferences = Preferences::open(path.clone());
    assert_eq!(preferences.error, Some("preferences_read_failed"));
    preferences.save();
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn rejects_invalid_opacity() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    let data = Data {
        surfaces: Some(Surfaces {
            main: 44.,
            sidebar: Some(100.),
        }),
        ..Default::default()
    };
    let bytes = serde_json::to_vec(&data).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let mut preferences = Preferences::open(path.clone());
    assert_eq!(preferences.error, Some("preferences_read_failed"));
    preferences.save();
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn ignores_obsolete_fields() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    let mut data = Data::default();
    data.terminal.font_size = 18;
    let mut value = serde_json::to_value(data).unwrap();
    value["workspaces"] = serde_json::json!({"invalid": "layout"});
    value["recent"] = serde_json::json!([{"invalid": "history"}]);
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();

    let mut preferences = Preferences::open(path.clone());
    assert_eq!(preferences.error, None);
    assert_eq!(preferences.data.terminal.font_size, 18);
    preferences.data.toast_seconds = 8;
    preferences.save();
    assert_eq!(preferences.error, None);

    let restored = Preferences::open(path);
    assert_eq!(restored.data.toast_seconds, 8);
    assert_eq!(restored.data.terminal.font_size, 18);
    let saved = serde_json::to_value(restored.data).unwrap();
    assert!(saved.get("workspaces").is_none());
    assert!(saved.get("recent").is_none());
}

#[gpui_kit::test]
fn session_pinning(cx: &mut gpui_kit::TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    let first = sailry_protocol::NodeId([1; 32]);
    let second = sailry_protocol::NodeId([2; 32]);
    let session = sailry_protocol::SessionId::new();
    cx.update(|cx| {
        cx.set_global(Preferences::open(path.clone()));
        sessions::set(first, session, sessions::State { pinned: true }, cx);
        assert_eq!(
            sessions::get(second, session, cx),
            sessions::State::default()
        );
        cx.set_global(Preferences::open(path));
        assert_eq!(
            sessions::get(first, session, cx),
            sessions::State { pinned: true }
        );
        sessions::set(first, session, sessions::State { pinned: true }, cx);
        assert!(sessions::get(first, session, cx).pinned);
        sessions::set(first, session, sessions::State::default(), cx);
        assert!(
            cx.global::<Preferences>()
                .data
                .sessions
                .as_ref()
                .unwrap()
                .is_empty()
        );
    });
}

mod plugin_directories {
    use super::*;
    use sailry_protocol::NodeId;

    #[gpui_kit::test]
    fn roundtrip_preserves_preferences(cx: &mut gpui_kit::TestAppContext) {
        let temporary = tempfile::tempdir().unwrap();
        let file = temporary.path().join("preferences.json");
        let source = temporary.path().join("package");
        let node = NodeId([1; 32]);
        cx.update(|cx| {
            cx.set_global(Preferences::open(file.clone()));
            update(cx, |data| {
                data.toast_seconds = 8;
                data.notifications[2] = false;
                data.terminal.font_size = 18;
            });
            let baseline = data(cx);
            assert_eq!(plugins::directory(node, "example", "digest", cx), None);
            plugins::remember(node, "example", "digest", 1, source.clone(), cx);
            assert_eq!(cx.global::<Preferences>().error, None);
            cx.set_global(Preferences::open(file));
            assert_eq!(
                plugins::directory(node, "example", "digest", cx),
                Some(source)
            );
            let mut restored = data(cx);
            assert_eq!(restored.version, 1);
            restored.plugin_directories = None;
            assert_eq!(restored, baseline);
        });
    }

    #[gpui_kit::test]
    fn isolates_node_name_and_digest(cx: &mut gpui_kit::TestAppContext) {
        let first = NodeId([1; 32]);
        let second = NodeId([2; 32]);
        let first_path = PathBuf::from("first-package");
        let second_path = PathBuf::from("second-package");
        let other_path = PathBuf::from("other-package");
        cx.update(|cx| {
            plugins::remember(first, "example", "first", 1, first_path.clone(), cx);
            plugins::remember(second, "example", "second", 1, second_path.clone(), cx);
            plugins::remember(first, "other", "other", 1, other_path.clone(), cx);
            assert_eq!(
                plugins::directory(first, "example", "first", cx),
                Some(first_path)
            );
            assert_eq!(
                plugins::directory(second, "example", "second", cx),
                Some(second_path)
            );
            assert_eq!(
                plugins::directory(first, "other", "other", cx),
                Some(other_path)
            );
            assert_eq!(plugins::directory(first, "missing", "first", cx), None);
            assert_eq!(plugins::directory(first, "example", "second", cx), None);
            let replacement = PathBuf::from("replacement-package");
            plugins::remember(first, "example", "next", 2, replacement.clone(), cx);
            assert_eq!(plugins::directory(first, "example", "first", cx), None);
            assert_eq!(
                plugins::directory(first, "example", "next", cx),
                Some(replacement)
            );
        });
    }

    #[gpui_kit::test]
    fn forgets_only_selected_package(cx: &mut gpui_kit::TestAppContext) {
        let temporary = tempfile::tempdir().unwrap();
        let file = temporary.path().join("preferences.json");
        let first = NodeId([1; 32]);
        let second = NodeId([2; 32]);
        let retained = temporary.path().join("retained-package");
        cx.update(|cx| {
            cx.set_global(Preferences::open(file.clone()));
            plugins::remember(
                first,
                "example",
                "first",
                1,
                temporary.path().join("package"),
                cx,
            );
            plugins::remember(second, "example", "second", 1, retained.clone(), cx);
            plugins::forget(first, "example", 1, cx);
            plugins::forget(first, "example", 1, cx);
            cx.set_global(Preferences::open(file));
            assert_eq!(plugins::directory(first, "example", "first", cx), None);
            assert_eq!(
                plugins::directory(second, "example", "second", cx),
                Some(retained)
            );
            plugins::forget(second, "example", 1, cx);
            assert!(
                cx.global::<Preferences>()
                    .data
                    .plugin_directories
                    .as_ref()
                    .unwrap()
                    .is_empty()
            );
        });
    }

    #[gpui_kit::test]
    fn ignores_delayed_success(cx: &mut gpui_kit::TestAppContext) {
        let temporary = tempfile::tempdir().unwrap();
        let file = temporary.path().join("preferences.json");
        let node = NodeId([1; 32]);
        let current = temporary.path().join("current-package");
        let previous = temporary.path().join("previous-package");
        cx.update(|cx| {
            cx.set_global(Preferences::open(file.clone()));
            plugins::remember(node, "example", "current", 4, current.clone(), cx);
            plugins::remember(node, "example", "current", 3, previous.clone(), cx);
            plugins::remember(node, "example", "previous", 2, previous, cx);
            plugins::forget(node, "example", 3, cx);
            assert_eq!(
                plugins::directory(node, "example", "current", cx),
                Some(current.clone())
            );
            assert_eq!(plugins::directory(node, "example", "previous", cx), None);

            // A newer install advances the guard even without content or path changes.
            plugins::remember(node, "example", "current", 5, current.clone(), cx);
            cx.set_global(Preferences::open(file));
            plugins::forget(node, "example", 4, cx);
            assert_eq!(
                plugins::directory(node, "example", "current", cx),
                Some(current)
            );
            plugins::forget(node, "example", 5, cx);
            assert_eq!(plugins::directory(node, "example", "current", cx), None);
        });
    }
}

#[test]
fn persists_selected_speech_model() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    let mut preferences = Preferences::open(path.clone());
    let id = sailry_protocol::ModelId::new();
    preferences.data.dictation = Some(Dictation {
        model: Some(id),
        ..Default::default()
    });
    preferences.save();
    assert_eq!(preferences.error, None);
    assert_eq!(
        Preferences::open(path).data.dictation.unwrap().model,
        Some(id)
    );
}

#[test]
fn missing_speech_selection_uses_default() {
    let options: Dictation =
        serde_json::from_str(r#"{"microphone":null,"language":"auto"}"#).unwrap();
    assert_eq!(options.model, None);
}

mod welcome {
    use super::*;

    #[test]
    fn starts_pending_after_logging_creates_directories() {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("new");
        std::fs::create_dir_all(profile.join("logs/desktop")).unwrap();
        let preferences = Preferences::for_startup(&profile);
        assert!(preferences.welcome_pending());
        assert_eq!(preferences.data.version, 1);
        assert_eq!(preferences.error, None);
        assert!(Preferences::open(profile.join("desktop/preferences.json")).welcome_pending());
    }

    #[test]
    fn preserves_pending_until_dismissed() {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path();
        Preferences::for_startup(profile);
        std::fs::create_dir_all(profile.join("storage")).unwrap();
        std::fs::write(
            profile.join("storage/node.sqlite3"),
            b"existing Node storage",
        )
        .unwrap();
        let mut restored = Preferences::for_startup(profile);
        assert!(restored.welcome_pending());
        restored.mark_welcome_seen();
        assert!(!restored.welcome_pending());
        assert_eq!(restored.error, None);
        let restored = Preferences::for_startup(profile);
        assert_eq!(restored.data.welcome_seen, Some(true));
        assert!(!restored.welcome_pending());
    }

    #[test]
    fn skips_existing_preferences_without_marker() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("desktop/preferences.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut expected = Data {
            toast_seconds: 8,
            ..Default::default()
        };
        expected.notifications[0] = false;
        let mut old = serde_json::to_value(&expected).unwrap();
        old.as_object_mut().unwrap().remove("welcome_seen");
        std::fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
        let preferences = Preferences::for_startup(directory.path());
        expected.welcome_seen = Some(true);
        assert_eq!(preferences.data, expected);
        assert!(!preferences.welcome_pending());
        assert_eq!(Preferences::open(path).data, expected);
    }

    #[test]
    fn skips_existing_node_without_preferences() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("storage/node.sqlite3");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = b"existing Node storage";
        std::fs::write(&path, original).unwrap();
        let preferences = Preferences::for_startup(directory.path());
        assert_eq!(preferences.data.welcome_seen, Some(true));
        assert!(!preferences.welcome_pending());
        assert_eq!(std::fs::read(path).unwrap(), original);
    }

    #[test]
    fn leaves_unreadable_preferences_unchanged() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("desktop/preferences.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = b"{invalid preferences";
        std::fs::write(&path, original).unwrap();
        let mut preferences = Preferences::for_startup(directory.path());
        assert_eq!(preferences.error, Some("preferences_read_failed"));
        assert!(!preferences.welcome_pending());
        preferences.mark_welcome_seen();
        assert_eq!(std::fs::read(path).unwrap(), original);
    }

    #[test]
    fn preview_never_becomes_pending() {
        let mut preferences = Preferences::default();
        preferences.data.welcome_seen = Some(false);
        assert!(!preferences.welcome_pending());
        preferences.mark_welcome_seen();
        assert_eq!(preferences.data.welcome_seen, Some(false));
        assert_eq!(preferences.path, None);
    }
}
