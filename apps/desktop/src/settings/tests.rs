use super::*;
use core::prelude::v1::test;

mod configuration;
mod conversation;
mod delegation;
mod media;
mod memory;
mod statistics;

#[gpui::test]
fn permissions_catalog_skips_preview_requests(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Workspace::new(window, cx));
        owner = Some(view.clone());
        Root::new(view, window, cx)
    });
    owner.unwrap().update(visual, |workspace, cx| {
        workspace.select(Section::Permissions, cx)
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    for id in [
        "permission_full_disk-row",
        "permission_accessibility-row",
        "permission_screen-row",
        "permission_microphone-row",
    ] {
        assert!(visual.debug_bounds(id).is_some());
    }
    assert!(visual.debug_bounds("permission_chrome-row").is_none());
    assert!(visual.debug_bounds("permission_keychain-row").is_none());
    assert!(visual.debug_bounds("permissions-continue").is_none());
    assert!(visual.opened_url().is_none());
    assert!(!visual.did_prompt_for_paths());
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
}

#[gpui::test]
fn page_headers(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Workspace::new(window, cx));
        owner = Some(view.clone());
        Root::new(view, window, cx)
    });
    let owner = owner.unwrap();
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [420., 960.] {
            let handle = visual.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            visual.simulate_window_resize(handle, size(px(width), px(820.)));
            for section in Section::ALL {
                visual.update(|window, cx| {
                    owner.update(cx, |owner, cx| owner.select(section, cx));
                    window.draw(cx).clear(cx);
                });
                visual.run_until_parked();
                visual.update(|window, cx| window.draw(cx).clear(cx));
                let header = visual.debug_bounds("settings-header").unwrap();
                let title = visual.debug_bounds("settings-heading").unwrap();
                let description = visual.debug_bounds("settings-description").unwrap();
                let viewport = visual.debug_bounds("settings-viewport").unwrap();
                let body = visual.debug_bounds("settings-content").unwrap();
                assert_eq!(
                    body.size.width,
                    px(width.min(crate::preview::CONTENT_WIDTH))
                );
                assert_eq!(body.center().x, viewport.center().x);
                assert_eq!(header.top(), title.top(), "{section:?}");
                assert!(description.top() > title.bottom(), "{section:?}");
                assert_eq!(header.bottom(), description.bottom(), "{section:?}");
                assert!(description.right() <= px(width), "{section:?}");
                let key = format!("{}_description", section.key());
                assert_ne!(tr(&key).as_ref(), key, "{section:?}");
            }
        }
    }
}

#[gpui::test]
fn empty_lists(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Workspace::new(window, cx));
        view.update(cx, |workspace, _| {
            workspace.roles.clear();
            workspace.skills.clear();
            workspace.plugins.clear();
            workspace.providers.channels.clear();
        });
        owner = Some(view.clone());
        Root::new(view, window, cx)
    });
    let owner = owner.unwrap();
    for (section, label, action) in [
        (Section::Skills, "empty-settings_empty", "skills-install"),
        (Section::Mcp, "empty-settings_empty", "mcp-open-plugins"),
        (Section::Plugins, "empty-plugins_none", "plugins-install"),
        (
            Section::Providers,
            "empty-provider_empty_live",
            "settings-add",
        ),
    ] {
        visual.update(|window, cx| {
            owner.update(cx, |owner, cx| owner.select(section, cx));
            window.draw(cx).clear(cx);
        });
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let empty = visual.debug_bounds(label).unwrap();
        let action = visual.debug_bounds(action).unwrap();
        let group = match section {
            Section::Skills => "settings-group-skills_installed",
            Section::Mcp => "settings-group-mcp_servers",
            Section::Plugins => "settings-group-plugins_installed",
            Section::Providers => "settings-group-provider_channels",
            _ => unreachable!(),
        };
        let card = visual.debug_bounds(group).unwrap();
        assert!(
            empty.left() >= card.left()
                && empty.top() >= card.top()
                && empty.right() <= card.right()
                && empty.bottom() <= card.bottom(),
            "{section:?}"
        );
        assert!(empty.size.height >= px(120.), "{section:?}: {empty:?}");
        assert!(action.bottom() <= empty.top(), "{section:?}");
    }
}

#[gpui::test]
fn model_catalog(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| crate::shell::Shell::new(window, cx));
        shell.update(cx, |shell, cx| {
            shell.settings.update(cx, |settings, _| {
                let mut channel = settings.providers.channels[0].clone();
                channel.id = 7;
                channel.models = vec![Model::example("another-preview")];
                settings.providers.channels.push(channel);
            });
            shell.conversations.get_mut(&(0, 2)).unwrap().options.model =
                Some(crate::conversation::models::Selection {
                    channel: 7,
                    model: "another-preview".into(),
                });
            assert_eq!(shell.selected_model(cx).unwrap().0.id, 7);
            shell.settings.update(cx, |settings, _| {
                settings.providers.channels[1].enabled = false
            });
            assert!(shell.selected_model(cx).is_none());
            assert_eq!(
                shell.conversations[&(0, 2)]
                    .options
                    .model
                    .as_ref()
                    .unwrap()
                    .channel,
                7
            );
        });
        Root::new(shell, window, cx)
    });
    visual.update(|window, cx| {
        _ = window.draw(cx);
    });
}

#[gpui::test]
fn retains_preferences(cx: &mut TestAppContext) {
    let profile = tempfile::tempdir().unwrap();
    let path = profile.path().join("preferences.json");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        cx.set_global(crate::preferences::Preferences::open(path.clone()));
    });
    let mut workspace = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Workspace::new(window, cx));
        workspace = Some(view.clone());
        Root::new(view, window, cx)
    });
    let workspace = workspace.unwrap();
    visual.update(|window, cx| {
        _ = window.draw(cx);
    });
    let before = rust_i18n::locale().to_string();
    let language = visual.debug_bounds("settings-language").unwrap();
    visual.simulate_click(language.center(), Modifiers::default());
    visual.simulate_keystrokes("down enter");
    visual.run_until_parked();
    assert_eq!(
        visual.update(|_, cx| crate::preferences::data(cx).language),
        Some(crate::locale::Language::English)
    );
    assert_eq!(rust_i18n::locale().to_string(), before);
    visual.update(|window, cx| {
        assert_eq!(
            crate::feedback::tests::summary(window, cx),
            tr("settings_language_restart")
        );
    });
    let duration = visual.debug_bounds("toast-duration").unwrap();
    visual.simulate_click(duration.center(), Modifiers::default());
    visual.simulate_keystrokes("down enter");
    visual.run_until_parked();
    assert_eq!(
        visual.update(|_, cx| crate::preferences::data(cx).toast_seconds),
        8
    );
    let toggle = visual
        .debug_bounds("toggle-settings_notify_system")
        .unwrap();
    visual.simulate_click(toggle.center(), Modifiers::default());
    visual.run_until_parked();
    visual.update(|_, cx| {
        workspace.update(cx, |settings, cx| {
            assert!(!crate::preferences::data(cx).notifications[0]);
            settings.select(Section::Appearance, cx);
            settings.select(Section::General, cx);
            assert!(!crate::preferences::data(cx).notifications[0]);
        })
    });
    assert!(
        visual
            .debug_bounds("settings-group-settings_pairing")
            .is_none()
    );
    let restored = crate::preferences::Preferences::open(path).data;
    assert_eq!(restored.language, Some(crate::locale::Language::English));
    assert_eq!(restored.toast_seconds, 8);
    assert!(!restored.notifications[0]);
}

#[gpui::test]
fn entry_slots(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut workspace = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Workspace::new(window, cx));
        workspace = Some(view.clone());
        Root::new(view, window, cx)
    });
    let workspace = workspace.unwrap();
    visual.update(|_, cx| {
        workspace.update(cx, |settings, cx| {
            settings.providers.channels[0].name = "Long provider name ".repeat(30);
            let mut other = settings.providers.channels[0].clone();
            other.id = 7;
            settings.providers.channels.push(other);
            settings.plugins[0].id = "long-plugin-name.".repeat(30);
            settings.roles[0].name = "Long role name ".repeat(30);
            settings.skills[0].managed = true;
            settings.skills[0].url = "https://example.invalid/long-path/".repeat(30);
            settings.terminal.preview.update(cx, |host, _| {
                host.environment
                    .push(("PREVIEW_VARIABLE".into(), "value".repeat(100)));
            });
        });
    });
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [420., 960.] {
            let handle = visual.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            visual.simulate_window_resize(handle, size(px(width), px(820.)));
            for (section, row, content, control, actions) in [
                (
                    Section::Providers,
                    "provider-0",
                    "provider-0-content",
                    Some("provider-0-control"),
                    "provider-0-actions",
                ),
                (
                    Section::Plugins,
                    "plugin-0",
                    "plugin-0-content",
                    Some("plugin-0-control"),
                    "plugin-0-actions",
                ),
                (
                    Section::Skills,
                    "skill-0",
                    "skill-0-content",
                    Some("skill-0-control"),
                    "skill-0-actions",
                ),
                (
                    Section::Terminal,
                    "environment-0",
                    "environment-0-content",
                    None,
                    "environment-0-actions",
                ),
                (
                    Section::Mcp,
                    "mcp-0",
                    "mcp-0-content",
                    None,
                    "mcp-0-actions",
                ),
            ] {
                visual.update(|_, cx| {
                    workspace.update(cx, |settings, cx| settings.select(section, cx))
                });
                visual.run_until_parked();
                visual.update(|window, cx| {
                    _ = window.draw(cx);
                });
                visual.run_until_parked();
                visual.update(|window, cx| {
                    _ = window.draw(cx);
                });
                if section == Section::Providers {
                    let divider = visual
                        .debug_bounds("settings-divider-provider_channels-1")
                        .unwrap();
                    let group = visual
                        .debug_bounds("settings-group-provider_channels")
                        .unwrap();
                    assert_eq!(divider.size.height, px(1.));
                    assert_eq!(divider.left(), group.left());
                    assert_eq!(divider.right(), group.right());
                }
                let row = visual.debug_bounds(row).unwrap();
                let content = visual.debug_bounds(content).unwrap();
                let actions = visual.debug_bounds(actions).unwrap();
                assert!((actions.right() - row.right()).abs() < px(1.));
                assert!(actions.right() <= px(width));
                assert!(content.right() < actions.left());
                assert!((actions.center().y - content.center().y).abs() < px(1.));
                assert!(actions.size.height < px(32.));
                let backgrounds = |window: &Window| {
                    let sample = (row.origin + point(px(1.), px(1.))).scale(window.scale_factor());
                    window
                        .painted_quads()
                        .into_iter()
                        .filter(|quad| quad.bounds.contains(&sample))
                        .map(|quad| quad.background)
                        .collect::<Vec<_>>()
                };
                visual.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
                visual.run_until_parked();
                let before = visual.update(|window, cx| {
                    _ = window.draw(cx);
                    backgrounds(window)
                });
                visual.simulate_mouse_move(content.center(), None, Modifiers::default());
                visual.run_until_parked();
                let hovered = visual.update(|window, cx| {
                    _ = window.draw(cx);
                    backgrounds(window)
                });
                assert_eq!(before, hovered, "Record hover must not paint a background");
                if let Some(control) = control {
                    let control = visual.debug_bounds(control).unwrap();
                    assert!(content.right() < control.left());
                    assert!(control.right() < actions.left());
                    assert!((control.center().y - actions.center().y).abs() < px(1.));
                    visual.simulate_click(control.center(), Modifiers::default());
                    visual.run_until_parked();
                    visual.update(|window, cx| {
                        let settings = workspace.read(cx);
                        let enabled = match section {
                            Section::Providers => settings.providers.channels[0].enabled,
                            Section::Plugins => settings.plugins[0].enabled,
                            Section::Skills => settings.skills[0].enabled,
                            _ => unreachable!(),
                        };
                        assert!(!enabled);
                        assert!(!window.has_active_dialog(cx));
                    });
                    visual.update(|window, cx| {
                        _ = window.draw(cx);
                    });
                    visual.simulate_click(control.center(), Modifiers::default());
                    visual.run_until_parked();
                }
            }
        }
    }
}
