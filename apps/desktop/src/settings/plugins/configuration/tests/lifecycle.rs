use super::*;

#[gpui::test]
fn retries_original_request(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture, "TOKEN", 0);
        let (owner, visual) = fixture.mount(cx);
        let editor = open(&fixture, &owner, visual);
        input(visual, "plugin-setting-label", "Original Node");
        fixture.transport.mode.store(1, Ordering::SeqCst);
        tap(visual, "plugin-settings-save");
        wait(visual, |cx| {
            !editor.read(cx).pending && editor.read(cx).request.is_some()
        });
        let id = editor.read_with(visual, |editor, _| editor.request.as_ref().unwrap().id);
        assert!(editor.read_with(visual, |editor, cx| {
            editor.fields.iter().all(|field| field.read(cx).locked)
        }));
        fixture.bind(&owner, visual, true);
        tap(visual, "plugin-settings-save");
        shown(visual, "plugin-settings-form", false);
        assert_eq!(*fixture.transport.requests.lock().unwrap(), [id, id]);
        assert_eq!(state(&fixture).values["label"], "Original Node");
        assert_eq!(state(&fixture).package.settings_revision, 1);
        let client = sailry_client::Client::new(fixture.other.local());
        let Output::Plugins(plugins) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::ListPlugins)))
            .unwrap()
        else {
            panic!("plugins expected");
        };
        assert_eq!(plugins, fixture.initial_packages);
        fixture.close(visual);
    }
}

#[gpui::test]
fn requires_binding_intent(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture, "TOKEN", 0);
        let initial = state(&fixture);
        fixture.execute(Command::SavePluginSettings {
            package: initial.package,
            values: initial.values,
            secrets: BTreeMap::from([(
                "token".into(),
                SecretUpdate::Replace(Secret::new("fixture-private".into())),
            )]),
        });
        let (owner, visual) = fixture.mount(cx);
        let editor = open(&fixture, &owner, visual);
        input(visual, "plugin-setting-label", "Keep this draft");
        install(&fixture, "CHANGED_TOKEN", fixture.plugins()[0].revision);
        tap(visual, "plugin-settings-reload");
        wait(visual, |cx| !editor.read(cx).loading);
        assert!(editor.read_with(visual, |editor, _| editor.changed));
        assert!(state(&fixture).keepable.is_empty());
        tap(visual, "plugin-settings-save");
        assert_eq!(
            editor.read_with(visual, |editor, _| editor.error),
            Some("plugins_settings_secret_intent")
        );
        next(visual, "plugin-intent-token");
        shown(visual, "plugin-secret-token", true);
        visual.simulate_event(ScrollWheelEvent {
            position: point(px(600.), px(300.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-300.))),
            touch_phase: TouchPhase::Moved,
            modifiers: Modifiers::default(),
        });
        draw(visual);
        input(visual, "plugin-secret-token", "fixture-new-private");
        tap(visual, "plugin-settings-save");
        wait(visual, |cx| !editor.read(cx).pending);
        assert_eq!(editor.read_with(visual, |editor, _| editor.error), None);
        shown(visual, "plugin-settings-form", false);
        assert_eq!(state(&fixture).values["label"], "Keep this draft");
        assert!(state(&fixture).ready);
        fixture.close(visual);
    }
}

#[gpui::test]
fn preserves_replacement(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture, "TOKEN", 0);
        let (owner, visual) = fixture.mount(cx);
        let editor = open(&fixture, &owner, visual);
        fixture.transport.mode.store(2, Ordering::SeqCst);
        tap(visual, "plugin-settings-save");
        wait(visual, |_| fixture.transport.entered.is_cancelled());
        tap(visual, "plugin-settings-cancel");
        let next = open(&fixture, &owner, visual);
        fixture.transport.release.cancel();
        wait(visual, |cx| !editor.read(cx).pending);
        assert!(editor.read_with(visual, |editor, _| editor.closed));
        assert!(!next.read_with(visual, |editor, _| editor.closed));
        shown(visual, "plugin-settings-form", true);
        tap(visual, "plugin-settings-cancel");
        fixture.close(visual);
    }
}

#[gpui::test]
fn releases_invalid_forms(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture, "TOKEN", 0);
        let (owner, visual) = fixture.mount(cx);
        let editor = open(&fixture, &owner, visual);
        next(visual, "plugin-intent-token");
        input(visual, "plugin-secret-token", "discarded-private-draft");
        let fields = editor.read_with(visual, |editor, _| editor.fields.clone());
        std::fs::write(
            fixture
                .directory
                .path()
                .join("project/package/dev.sailry.platform/settings.json"),
            "{}",
        )
        .unwrap();
        fixture.execute(Command::InstallPlugin {
            worktree: fixture.worktree,
            path: "package".into(),
            name: "example".into(),
            expected_revision: fixture.plugins()[0].revision,
        });
        tap(visual, "plugin-settings-reload");
        wait(visual, |cx| !editor.read(cx).loading);
        assert!(editor.read_with(visual, |editor, _| editor.info.is_none()
            && editor.fields.is_empty()));
        assert!(editor.read_with(visual, |editor, cx| editor.prepare(cx).is_err()));
        assert!(visual.update(|_, cx| fields.iter().all(|field| !matches!(
            field.read(cx).value(cx),
            Ok(fields::Value::Secret(SecretUpdate::Replace(_)))
        ))));
        tap(visual, "plugin-settings-cancel");
        fixture.close(visual);
    }
}

#[gpui::test]
fn scrolls_long_forms(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture, "TOKEN", 0);
        let path = fixture
            .directory
            .path()
            .join("project/package/dev.sailry.platform/settings.json");
        let mut schema: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for index in 1..=27 {
            schema["properties"][format!("option{index:02}")] =
                json!({"type":"string","default":"Fixture value"});
        }
        std::fs::write(&path, schema.to_string()).unwrap();
        fixture.execute(Command::InstallPlugin {
            worktree: fixture.worktree,
            path: "package".into(),
            name: "example".into(),
            expected_revision: fixture.plugins()[0].revision,
        });
        let (owner, visual) = fixture.mount(cx);
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(760.), px(560.)));
        let editor = open(&fixture, &owner, visual);
        assert_eq!(
            editor.read_with(visual, |editor, _| editor.fields.len()),
            32
        );
        let save = visual.debug_bounds("plugin-settings-save").unwrap();
        assert!(save.bottom() < px(560.));
        visual.simulate_event(ScrollWheelEvent {
            position: point(px(600.), px(300.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-10000.))),
            touch_phase: TouchPhase::Moved,
            modifiers: Modifiers::default(),
        });
        draw(visual);
        let last = visual.debug_bounds("plugin-intent-token").unwrap();
        assert!(last.origin.y > px(0.) && last.bottom() <= save.origin.y);
        next(visual, "plugin-intent-token");
        visual.simulate_event(ScrollWheelEvent {
            position: point(px(600.), px(300.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-1000.))),
            touch_phase: TouchPhase::Moved,
            modifiers: Modifiers::default(),
        });
        draw(visual);
        input(visual, "plugin-secret-token", "long-form-private");
        tap(visual, "plugin-settings-save");
        shown(visual, "plugin-settings-form", false);
        assert!(state(&fixture).ready);
        assert_eq!(state(&fixture).values.len(), 30);
        fixture.close(visual);
    }
}
