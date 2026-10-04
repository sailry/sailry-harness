use super::super::test_support::{Fixture, draw, init, input, shown, tap, wait};
use super::*;
use core::prelude::v1::test;
use sailry_protocol::{Secret, plugin::settings::SecretUpdate};
use serde_json::json;
use std::{collections::BTreeMap, sync::atomic::Ordering};

mod lifecycle;
mod models;

fn install(fixture: &Fixture, binding: &str, revision: u64) -> Summary {
    fixture.package("1.0.0");
    let root = fixture.directory.path().join("project/package");
    std::fs::write(root.join("plugin.json"), json!({
        "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
        "name":"example", "version":"1.0.0",
        "extensions":{"dev.sailry.platform":{"api_version":"v1", "actions":[], "settings_schema":"dev.sailry.platform/settings.json"}}
    }).to_string()).unwrap();
    std::fs::write(root.join("mcp.json"), json!({
        "$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers":{"native":{"type":"stdio","command":"sailry_isolated_missing_executable", "env":{binding:""}}}
    }).to_string()).unwrap();
    std::fs::write(root.join("dev.sailry.platform/settings.json"), json!({
        "$schema": sailry_protocol::plugin::settings::SCHEMA, "type":"object", "additionalProperties":false,
        "properties": {
            "label":{"type":"string","title":"Label","default":"Original", "minLength":1},
            "limit":{"type":"integer","title":"Limit", "default":9007199254740993u64},
            "mode":{"type":"string","title":"Mode", "enum":["fast","complete"],"default":"fast"},
            "notify":{"type":"boolean","title":"Notify"},
            "token":{"type":"string","title":"Token","x-sailry-secret":{"server":"native","env":binding}}
        }, "required":["label","limit","mode","token"]
    }).to_string()).unwrap();
    let Output::Plugin(info) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.worktree,
        path: "package".into(),
        name: "example".into(),
        expected_revision: revision,
    }) else {
        panic!("plugin expected")
    };
    assert!(
        info.settings.is_some(),
        "invalid settings fixture: {:?}",
        info.issues
    );
    info.summary
}

fn open(
    fixture: &Fixture,
    owner: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> Entity<Editor> {
    let binding = owner.read_with(cx, |owner, _| {
        owner.provider_link.as_ref().unwrap().binding.clone()
    });
    let editor = cx.update(|window, cx| mount(binding, fixture.plugins().remove(0), window, cx));
    wait(cx, |cx| !editor.read(cx).loading);
    assert!(editor.read_with(cx, |editor, _| editor.info.is_some()));
    draw(cx);
    editor
}

fn state(fixture: &Fixture) -> State {
    let Output::PluginSettings(state) = fixture.execute(Command::ReadPluginSettings {
        package: fixture.plugins()[0].reference(),
    }) else {
        panic!("settings expected")
    };
    state
}

fn external(fixture: &Fixture, label: &str) {
    fixture.execute(Command::SavePluginSettings {
        package: fixture.plugins()[0].reference(),
        values: BTreeMap::from([
            ("label".into(), json!(label)),
            ("limit".into(), json!(42)),
            ("mode".into(), json!("fast")),
        ]),
        secrets: BTreeMap::from([("token".into(), SecretUpdate::Keep)]),
    });
}

fn next(cx: &mut VisualTestContext, selector: &'static str) {
    tap(cx, selector);
    cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
    cx.simulate_keystrokes("down enter");
    draw(cx);
}

#[gpui::test]
fn details_keep_the_captured_node(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let expected = install(&fixture, "TOKEN", 0);
        let (owner, visual) = fixture.mount(cx);
        visual
            .update(|window, cx| super::super::details::open(owner.clone(), expected, window, cx));
        shown(visual, "plugin-details-configure", true);
        fixture.bind(&owner, visual, true);
        tap(visual, "plugin-details-configure");
        shown(visual, "plugin-setting-label", true);
        input(visual, "plugin-setting-label", "Captured node");
        next(visual, "plugin-intent-token");
        input(visual, "plugin-secret-token", "fixture-private-token");
        tap(visual, "plugin-settings-save");
        shown(visual, "plugin-settings-form", false);
        assert_eq!(state(&fixture).values["label"], "Captured node");
        let other = fixture.public_packages(true);
        assert_eq!(
            owner.read_with(visual, |owner, _| owner.plugin_catalog.packages.clone()),
            other
        );
        visual.simulate_keystrokes("escape");
        fixture.close(visual);
    }
}

#[gpui::test]
fn saves_typed_values(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture, "TOKEN", 0);
        let (owner, visual) = fixture.mount(cx);
        let editor = open(&fixture, &owner, visual);
        input(visual, "plugin-setting-label", "Edited");
        input(visual, "plugin-setting-limit", "9007199254740995");
        next(visual, "plugin-setting-mode");
        tap(visual, "plugin-boolean-notify");
        next(visual, "plugin-intent-token");
        shown(visual, "plugin-secret-token", true);
        input(visual, "plugin-secret-token", "fixture-private-token");
        visual.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("sentinel".into())));
        visual.simulate_keystrokes("secondary-a secondary-c");
        assert_eq!(
            visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            "sentinel"
        );
        let command = editor.read_with(visual, |editor, cx| editor.prepare(cx).unwrap());
        assert!(!format!("{command:?}").contains("fixture-private-token"));
        tap(visual, "plugin-settings-save");
        shown(visual, "plugin-settings-form", false);
        let saved = state(&fixture);
        assert!(saved.ready);
        assert_eq!(saved.values["label"], "Edited");
        assert_eq!(saved.values["limit"], json!(9007199254740995u64));
        assert_eq!(saved.values["mode"], "complete");
        assert_eq!(saved.values["notify"], true);
        assert_eq!(saved.configured, ["token"]);
        assert!(
            editor.read_with(visual, |editor, cx| editor.fields.iter().all(
                |field| !matches!(
                    field.read(cx).value(cx),
                    Ok(fields::Value::Secret(SecretUpdate::Replace(_)))
                )
            ))
        );
        let editor = open(&fixture, &owner, visual);
        tap(visual, "plugin-boolean-notify");
        let Command::SavePluginSettings { values, .. } =
            editor.read_with(visual, |editor, cx| editor.prepare(cx).unwrap())
        else {
            panic!("save expected")
        };
        assert_eq!(values["notify"], false);
        tap(visual, "plugin-unset-notify");
        let command = editor.read_with(visual, |editor, cx| editor.prepare(cx).unwrap());
        assert!(
            matches!(command,Command::SavePluginSettings {secrets,..} if secrets["token"] == SecretUpdate::Keep)
        );
        next(visual, "plugin-intent-token");
        next(visual, "plugin-intent-token");
        tap(visual, "plugin-settings-save");
        shown(visual, "plugin-settings-form", false);
        assert!(!state(&fixture).ready);
        assert!(state(&fixture).configured.is_empty());
        assert!(!state(&fixture).values.contains_key("notify"));
        fixture.close(visual);
    }
}

#[gpui::test]
fn preserves_conflicting_drafts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture, "TOKEN", 0);
        let (owner, visual) = fixture.mount(cx);
        let editor = open(&fixture, &owner, visual);
        tap(visual, "plugin-setting-label");
        visual.simulate_keystrokes("secondary-a backspace");
        draw(visual);
        tap(visual, "plugin-settings-save");
        crate::feedback::tests::shown(visual);
        shown(visual, "plugin-settings-error", false);
        assert_eq!(state(&fixture).package.settings_revision, 0);
        input(visual, "plugin-setting-label", "My draft");
        input(visual, "plugin-setting-limit", "9007199254740995");
        external(&fixture, "Other client");
        tap(visual, "plugin-settings-save");
        wait(visual, |cx| {
            editor.read(cx).error == Some("plugins_settings_conflict")
        });
        tap(visual, "plugin-settings-reload");
        wait(visual, |cx| !editor.read(cx).loading);
        tap(visual, "plugin-settings-save");
        shown(visual, "plugin-settings-form", false);
        let saved = state(&fixture);
        assert_eq!(saved.values["label"], "My draft");
        assert_eq!(saved.values["limit"], json!(9007199254740995u64));
        assert_eq!(saved.package.settings_revision, 2);
        fixture.close(visual);
    }
}

#[gpui::test]
fn embedded_page_keeps_editing_after_save(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package("1.0.0");
        let root = fixture.directory.path().join("project/package");
        std::fs::write(root.join("plugin.json"), json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name":"example",
            "extensions":{"dev.sailry.platform":{"api_version":"v1", "actions":[], "settings_schema":"dev.sailry.platform/settings.json", "settings_page":{"navigation":{"label":"Example settings"}}}}
        }).to_string()).unwrap();
        std::fs::write(root.join("dev.sailry.platform/settings.json"), json!({
            "$schema": sailry_protocol::plugin::settings::SCHEMA, "type":"object", "additionalProperties":false,
            "properties":{"label":{"type":"string","title":"Label","default":"Original"}}
        }).to_string()).unwrap();
        let Output::Plugin(info) = fixture.execute(Command::InstallPlugin {
            worktree: fixture.worktree,
            path: "package".into(),
            name: "example".into(),
            expected_revision: 0,
        }) else {
            panic!("plugin expected")
        };
        let binding = Binding {
            client: std::sync::Arc::new(sailry_client::Client::new(fixture.transport.clone())),
            runtime: fixture.runtime.clone(),
            label: "Fixture".into(),
        };
        let mut editor = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = page(binding, info.summary, window, cx);
            editor = Some(view.clone());
            Root::new(view, window, cx)
        });
        let editor = editor.unwrap();
        wait(visual, |cx| {
            !editor.read(cx).loading && editor.read(cx).info.is_some()
        });
        for (revision, label) in [(1, "First"), (2, "Second")] {
            input(visual, "plugin-setting-label", label);
            tap(visual, "plugin-settings-save");
            wait(visual, |cx| {
                let state = editor.read(cx);
                !state.loading && !state.pending && state.expected.settings_revision == revision
            });
            assert!(!editor.read_with(visual, |editor, _| editor.closed));
            shown(visual, "plugin-settings-form", true);
            assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
            let Output::PluginSettings(state) = fixture.execute(Command::ReadPluginSettings {
                package: editor.read_with(visual, |editor, _| editor.expected.reference()),
            }) else {
                panic!("settings expected")
            };
            assert_eq!(state.values["label"], label);
        }
        drop(editor);
        fixture.close(visual);
    }
}
