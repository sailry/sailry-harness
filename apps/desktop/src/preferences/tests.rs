use super::*;

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
fn isolates_unreadable_layout() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    let mut data = Data::default();
    let layout = serde_json::json!({"invalid": "layout"});
    data.workspaces = Some(layout.clone());
    data.terminal.font_size = 18;
    std::fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();

    let mut preferences = Preferences::open(path.clone());
    assert_eq!(preferences.error, None);
    assert_eq!(preferences.data.terminal.font_size, 18);
    preferences.data.toast_seconds = 8;
    preferences.save();
    assert_eq!(preferences.error, None);

    let restored = Preferences::open(path);
    assert_eq!(restored.data.toast_seconds, 8);
    assert_eq!(restored.data.terminal.font_size, 18);
    assert_eq!(restored.data.workspaces, Some(layout));
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
                data.workspaces = Some(serde_json::json!({"fixture": "layout"}));
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
