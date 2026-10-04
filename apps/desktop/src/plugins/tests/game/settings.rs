use super::*;
use sailry_protocol::Output;

fn tap(visual: &mut VisualTestContext, selector: &'static str) {
    if visual.update(|window, cx| window.has_active_dialog(cx)) {
        std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
    }
    visual.update(|window, _| window.refresh());
    click(visual, selector);
}

#[track_caller]
fn shown(visual: &mut VisualTestContext, selector: &'static str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        if visual.debug_bounds(selector).is_some() {
            return;
        }
        assert!(Instant::now() < deadline, "missing {selector}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn edits_players_from_each_game_toolbar(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        for (name, fields, enabled, model_selector) in [
            (
                "city-trader",
                &["player_1_model", "player_2_model"][..],
                "player_1_enabled",
                "plugin-setting-player_1_model",
            ),
            (
                "liars-dice",
                &["player_1_model", "player_2_model"][..],
                "player_1_enabled",
                "plugin-setting-player_1_model",
            ),
            (
                "doudizhu",
                &["player_1_model", "player_2_model"][..],
                "player_1_enabled",
                "plugin-setting-player_1_model",
            ),
            (
                "poker",
                &["player_1_model"][..],
                "player_1_enabled",
                "plugin-setting-player_1_model",
            ),
            (
                "gomoku",
                &["ai_model"][..],
                "ai_enabled",
                "plugin-setting-ai_model",
            ),
            (
                "reversi",
                &["ai_model"][..],
                "ai_enabled",
                "plugin-setting-ai_model",
            ),
            (
                "xiangqi",
                &["ai_model"][..],
                "ai_enabled",
                "plugin-setting-ai_model",
            ),
        ] {
            let fixture = Fixture::new(remote);
            let (mut package, server) = fixture.game_plugin_with(name, fields, |root| {
                if name == "city-trader" {
                    let path = root.join("dev.sailry.platform/desktop/controls.js");
                    let source = std::fs::read_to_string(&path).unwrap();
                    let source = source.replace(
                        "return primary ? control.primary() : control.outline();",
                        "return TestAnchor.new(id).child(primary ? control.primary() : control.outline());",
                    );
                    std::fs::write(path, format!("import {{ Anchor as TestAnchor }} from \"sailry/test\";\n{source}")).unwrap();
                }
            });
            if name == "city-trader" {
                let Output::PluginSettings(mut state) =
                    fixture.execute(Command::ReadPluginSettings {
                        package: package.summary.reference(),
                    })
                else {
                    panic!("settings expected");
                };
                state.values.remove("player_1_model");
                state.values.remove("player_1_model_effort");
                fixture.execute(Command::SavePluginSettings {
                    package: state.package,
                    values: state.values,
                    secrets: Default::default(),
                });
                let Output::Plugin(updated) =
                    fixture.execute(Command::ReadPlugin { name: name.into() })
                else {
                    panic!("plugin expected");
                };
                package = updated;
            }
            assert!(package.extension.as_ref().unwrap().settings_page.is_none());
            assert_eq!(
                package.settings.as_ref().unwrap().tabs.len(),
                fields.len() + 1
            );
            assert!(package.settings.as_ref().unwrap().tabs.len() <= 3);
            cx.update(|cx| cx.set_global(fixture.services(false)));
            let mut owner = None;
            let (_, visual) = cx.add_window_view(|window, cx| {
                let shell = cx.new(|cx| Shell::new(window, cx));
                owner = Some(shell.clone());
                Root::new(shell, window, cx)
            });
            let shell = owner.unwrap();
            let handle = visual.update(|window, _| window.window_handle());
            visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
            wait(visual, |cx| {
                shell
                    .read(cx)
                    .extension_entries(cx)
                    .iter()
                    .any(|entry| entry.package == package.summary.reference())
            });
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    let entry = shell
                        .extension_entries(cx)
                        .into_iter()
                        .find(|entry| entry.package == package.summary.reference())
                        .unwrap();
                    shell.open_extension(entry, window, cx);
                })
            });
            let panel = shell.read_with(visual, |shell, _| {
                shell.extensions.as_ref().unwrap().panel.clone().unwrap()
            });
            wait(visual, |cx| !snapshot(&panel, cx).is_empty());
            shown(visual, "plugin-settings");
            let header = visual.debug_bounds("shell-module-header").unwrap();
            let settings = visual.debug_bounds("plugin-settings").unwrap();
            assert!(settings.top() >= header.top() && settings.bottom() <= header.bottom());
            assert!(settings.right() <= header.right());
            assert!(visual.debug_bounds("workspace-header").is_none());
            tap(visual, "plugin-settings");
            shown(visual, "plugin-setting-player_name");
            assert_eq!(shell.read_with(visual, |shell, _| shell.page), Page::Plugin);
            assert!(visual.debug_bounds("plugin-settings-reload").is_none());
            assert!(visual.debug_bounds("plugin-unset-player_name").is_none());
            tap(visual, "plugin-setting-player_name");
            visual.simulate_keystrokes("secondary-a");
            visual.simulate_input("Captain");
            tap(visual, "plugin-settings-tab-player-tab-2");
            shown(visual, model_selector);
            for field in ["player_name", fields[0]] {
                let selector = Box::leak(format!("plugin-unset-{field}").into_boxed_str());
                assert!(visual.debug_bounds(selector).is_none());
            }
            let type_selector = if fields[0] == "ai_model" {
                "plugin-model-type-ai_model"
            } else {
                "plugin-model-type-player_1_model"
            };
            assert!(visual.debug_bounds(type_selector).is_none());
            let picker_selector =
                Box::leak(format!("plugin-model-choice-{}", fields[0]).into_boxed_str());
            let picker = visual.debug_bounds(picker_selector).unwrap();
            let model = visual.debug_bounds(model_selector).unwrap();
            assert!(picker.top() >= model.top() && picker.bottom() <= model.bottom());
            assert!((picker.left() - model.left()).abs() <= px(1.));
            assert!((picker.size.width - model.size.width).abs() <= px(1.));
            let switch = if enabled == "ai_enabled" {
                "plugin-boolean-ai_enabled"
            } else {
                "plugin-boolean-player_1_enabled"
            };
            let toggle = visual.debug_bounds(switch).unwrap();
            assert!(
                toggle.top() > model.bottom(),
                "switches follow the model fields"
            );
            tap(visual, switch);
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert!(visual.debug_bounds(model_selector).is_none());
            if fields.len() == 2 {
                tap(visual, "plugin-settings-tab-player-tab-3");
                shown(visual, "plugin-setting-player_2_model");
            }
            assert!(
                visual
                    .debug_bounds("plugin-settings-tab-player-tab-4")
                    .is_none()
            );
            tap(visual, "plugin-settings-save");
            wait(visual, |cx| {
                panel.read(cx).selected.as_ref().is_some_and(|reference| {
                    reference.settings_revision > package.summary.settings_revision
                })
            });
            let Output::Plugin(updated) =
                fixture.execute(Command::ReadPlugin { name: name.into() })
            else {
                panic!("plugin expected");
            };
            let Output::PluginSettings(saved) = fixture.execute(Command::ReadPluginSettings {
                package: updated.summary.reference(),
            }) else {
                panic!("settings expected");
            };
            assert_eq!(saved.values["player_name"], "Captain");
            assert_eq!(saved.values[enabled], false);
            if name == "city-trader" {
                assert!(!saved.values.contains_key(fields[0]));
                assert_eq!(saved.values["player_1_model_effort"], "");
            } else {
                assert!(
                    saved.values[fields[0]]
                        .as_str()
                        .is_some_and(|value| !value.is_empty())
                );
            }
            if name == "city-trader" {
                wait(visual, |cx| {
                    snapshot(&panel, cx).contains("city-status-ready")
                });
                tap(visual, "city-start");
                wait(visual, |cx| {
                    snapshot(&panel, cx).contains("city-state-roll-0-1")
                });
                let tree = visual.update(|_, cx| snapshot(&panel, cx));
                assert!(!tree.contains("city-seat-1"));
                assert!(tree.contains("city-seat-2"));
            }
            shown(visual, "plugin-settings");
            tap(visual, "plugin-settings");
            shown(visual, "plugin-setting-player_name");
            tap(visual, "plugin-settings-tab-player-tab-2");
            shown(visual, switch);
            assert!(visual.debug_bounds(model_selector).is_none());
            tap(visual, "plugin-settings-cancel");
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert!(visual.debug_bounds("plugin-settings-form").is_none());
            assert_eq!(shell.read_with(visual, |shell, _| shell.page), Page::Plugin);
            if name == "city-trader" {
                assert!(
                    visual.update(|_, cx| snapshot(&panel, cx).contains("city-state-roll-0-1"))
                );
            }
            assert!(server.requests.lock().unwrap().is_empty());
            visual.update(|window, _| window.remove_window());
            drop(panel);
            drop(shell);
            fixture.close();
        }
    }
}
