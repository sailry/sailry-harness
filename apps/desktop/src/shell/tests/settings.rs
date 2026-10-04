use super::*;
use crate::settings::Section;

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(250));
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
}

fn open(shell: &Entity<Shell>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.navigate(Page::Settings, window, cx))
    });
    draw(cx);
}

#[gpui::test]
fn local_dictation_navigation(cx: &mut TestAppContext) {
    let (shell, mut visual) = setup(cx);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_dictation_settings(window, cx);
            assert_eq!(shell.page, Page::Settings);
            assert_eq!(shell.settings.read(cx).section, Section::Dictation);
            assert!(shell.settings_target.is_none());
        })
    });
}

#[gpui::test]
fn replaces_navigation(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    open(&shell, &mut cx);
    assert!(cx.debug_bounds("settings-back").is_none());
    let general = cx.debug_bounds("settings_general").unwrap();
    let nav = cx.debug_bounds("settings-navigation-viewport").unwrap();
    assert_eq!(nav.top(), px(HEADER_HEIGHT));
    assert!(general.left() >= nav.left());
    assert!(cx.debug_bounds("settings-content").is_some());
    for selector in ["sidebar-brand", "host-0", "session-0"] {
        assert!(cx.debug_bounds(selector).is_none());
    }
    assert!(cx.debug_bounds("navigation-files").is_some());
    assert!(cx.debug_bounds("sidebar-settings").is_some());
    assert!(cx.debug_bounds("settings_plugins").is_none());
    for section in Section::ALL
        .into_iter()
        .filter(|section| !section.is_extension())
    {
        // The AI section can extend below the viewport; navigate through the actual scroll area.
        for _ in 0..8 {
            let row = cx.debug_bounds(section.key()).unwrap();
            let nav = cx.debug_bounds("settings-navigation-viewport").unwrap();
            if row.center().y >= nav.top() && row.center().y < nav.bottom() {
                break;
            }
            cx.simulate_event(ScrollWheelEvent {
                position: nav.center(),
                delta: ScrollDelta::Pixels(point(
                    px(0.),
                    px(if row.center().y < nav.top() {
                        160.
                    } else {
                        -160.
                    }),
                )),
                ..Default::default()
            });
            draw(&mut cx);
        }
        let row = cx.debug_bounds(section.key()).unwrap();
        cx.simulate_click(row.center(), Modifiers::default());
        draw(&mut cx);
        assert_eq!(
            cx.update(|_, cx| shell.read(cx).settings.read(cx).section),
            section
        );
        let body = cx.debug_bounds("settings-content").unwrap();
        let viewport = cx.debug_bounds("settings-viewport").unwrap();
        assert_eq!(viewport.left(), px(NAV_WIDTH + RAIL_WIDTH));
        assert_eq!(body.center().x, viewport.center().x);
        assert!(body.left() >= viewport.left() && body.right() <= viewport.right());
        assert!(cx.debug_bounds("settings-preview-notice").is_none());
    }
}

#[gpui::test]
fn restores_workspace(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_host(1, window, cx);
            shell.conversations[&(1, 0)].input.update(cx, |input, cx| {
                input.set_value("retained draft", window, cx)
            });
            shell.navigate(Page::Files, window, cx);
            shell.layout.panel_open[Page::Files.panel_index()] = false;
        })
    });
    open(&shell, &mut cx);
    let files = cx.debug_bounds("navigation-files").unwrap();
    cx.simulate_click(files.center(), Modifiers::default());
    draw(&mut cx);
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.page, Page::Files);
        assert_eq!(shell.host, 1);
        assert!(!shell.layout.panel_open[Page::Files.panel_index()]);
        assert_eq!(
            shell.conversations[&(1, 0)].input.read(cx).value(),
            "retained draft"
        );
    });
}

#[gpui::test]
fn themes_and_sizes(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    open(&shell, &mut cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [760., 1280.] {
            let handle = cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(width), px(560.)));
            for section in Section::ALL {
                cx.update(|window, cx| {
                    shell.update(cx, |shell, cx| {
                        shell.navigate(
                            if section.is_extension() {
                                Page::Plugins
                            } else {
                                Page::Settings
                            },
                            window,
                            cx,
                        );
                        shell
                            .settings
                            .update(cx, |settings, cx| settings.select(section, cx));
                        cx.notify();
                    })
                });
                draw(&mut cx);
                assert!(cx.debug_bounds("settings-back").is_none());
                let body = cx.debug_bounds("settings-content").unwrap();
                if section.is_extension() {
                    assert!(cx.debug_bounds("settings-navigation").is_none());
                    let header = cx.debug_bounds("shell-module-header").unwrap();
                    for tab in [
                        "plugins-market-tab",
                        "plugins-manage-tab",
                        "plugins-skills-tab",
                        "plugins-mcp-tab",
                    ] {
                        let tab = cx.debug_bounds(tab).unwrap();
                        assert!(tab.left() >= header.left() && tab.right() <= header.right());
                    }
                } else {
                    let nav = cx.debug_bounds("settings-navigation").unwrap();
                    assert_eq!(nav.top(), px(HEADER_HEIGHT));
                }
                assert!(body.right() <= px(width));
                let viewport = cx.debug_bounds("settings-viewport").unwrap();
                // Kit's selector describes the intrinsic scroll content, not its clipped viewport.
                let clip = cx.debug_bounds("shell-body").unwrap();
                assert!(clip.bottom() <= px(560.), "{section:?}: {clip:?}");
                assert!(body.left() >= viewport.left() && body.right() <= viewport.right());
            }
        }
    }
}

#[gpui::test]
fn native_theme_control(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    open(&shell, &mut cx);
    let appearance = cx.debug_bounds("settings_appearance").unwrap();
    cx.simulate_click(appearance.center(), Modifiers::default());
    draw(&mut cx);
    for width in [760., 1280.] {
        let handle = cx.update(|window, _| window.window_handle());
        cx.simulate_window_resize(handle, size(px(width), px(560.)));
        draw(&mut cx);
        for (key, mode) in [
            ("appearance_dark", Some(ThemeMode::Dark)),
            ("appearance_light", Some(ThemeMode::Light)),
            ("appearance_system", None),
            ("appearance_light", Some(ThemeMode::Light)),
        ] {
            let tab = cx.debug_bounds(key).unwrap();
            let body = cx.debug_bounds("settings-content").unwrap();
            assert!(tab.left() >= body.left() && tab.right() <= body.right());
            cx.simulate_click(tab.center(), Modifiers::default());
            draw(&mut cx);
            cx.update(|_, cx| {
                assert_eq!(crate::theme::follows_system(cx), mode.is_none());
                if let Some(mode) = mode {
                    assert_eq!(cx.theme().mode, mode);
                }
            });
        }
    }
}

#[gpui::test]
fn group_actions(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    open(&shell, &mut cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [760., 1280.] {
            let handle = cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(width), px(820.)));
            for (section, header, actions, button) in [
                (
                    Section::Skills,
                    "settings-heading-skills_installed",
                    "settings-actions-skills_installed",
                    "skills-install",
                ),
                (
                    Section::Plugins,
                    "settings-heading-plugins_installed",
                    "settings-actions-plugins_installed",
                    "plugins-install",
                ),
                (
                    Section::Mcp,
                    "settings-heading-mcp_servers",
                    "settings-actions-mcp_servers",
                    "mcp-open-plugins",
                ),
                (
                    Section::Appearance,
                    "settings-heading-appearance_packages",
                    "settings-actions-appearance_packages",
                    "theme-import",
                ),
                (
                    Section::Terminal,
                    "settings-heading-terminal_environment",
                    "settings-actions-terminal_environment",
                    "environment-add",
                ),
                (
                    Section::Providers,
                    "settings-heading-provider_channels",
                    "settings-actions-provider_channels",
                    "settings-add",
                ),
            ] {
                cx.update(|_, cx| {
                    shell
                        .read(cx)
                        .settings
                        .clone()
                        .update(cx, |settings, cx| settings.select(section, cx));
                });
                draw(&mut cx);
                let custom_title = button == "settings-add";
                let header = cx.debug_bounds(header).unwrap_or_else(|| {
                    panic!("missing {header} in {section:?} at {width}px ({mode:?})")
                });
                let actions = cx.debug_bounds(actions).unwrap_or_else(|| {
                    panic!("missing {actions} in {section:?} at {width}px ({mode:?})")
                });
                let button = cx.debug_bounds(button).unwrap_or_else(|| {
                    panic!("missing {button} in {section:?} at {width}px ({mode:?})")
                });
                let body = cx.debug_bounds("settings-content").unwrap();
                if custom_title {
                    assert_eq!(header.size.height, px(36.));
                } else {
                    assert!(header.size.height >= px(28.) && header.size.height <= px(32.));
                }
                assert!((actions.right() - header.right()).abs() < px(1.));
                assert!(actions.left() >= header.left());
                assert!(actions.top() >= header.top() && actions.bottom() <= header.bottom());
                assert_eq!(button.size.height, px(32.));
                assert!(button.right() <= body.right());
                if section == Section::Skills {
                    let discover = cx.debug_bounds("skills-discover").unwrap();
                    assert_eq!(discover.size.height, button.size.height);
                    assert!(discover.right() < button.left());
                    cx.update(|window, cx| {
                        let quads = window.painted_quads();
                        let primary: Background = cx.theme().tokens.button_primary.into();
                        let scale = window.scale_factor();
                        assert!(quads.iter().any(|quad| {
                            quad.bounds == button.scale(scale) && quad.background == primary
                        }));
                        assert!(!quads.iter().any(|quad| {
                            quad.bounds == discover.scale(scale) && quad.background == primary
                        }));
                    });
                }
                if custom_title {
                    assert!(cx.debug_bounds("models-dev-refresh").is_none());
                    let title = cx.debug_bounds("settings-title-provider_channels").unwrap();
                    assert!((title.left() - header.left()).abs() < px(1.));
                    assert!(title.right() < actions.left());
                    assert!((title.center().y - actions.center().y).abs() < px(1.));
                    let rows = cx.debug_bounds("settings-group-provider_channels").unwrap();
                    let gap = rows.top() - header.bottom();
                    assert!(gap >= px(8.) && gap <= px(10.));
                    let picker = cx.debug_bounds("provider-families").unwrap();
                    assert_eq!(picker.size.height, px(36.));
                    assert_eq!(
                        cx.debug_bounds("provider_openai").unwrap().size.height,
                        px(32.)
                    );
                    cx.update(|window, _| assert_eq!(window.rem_size(), px(16.)));
                    assert!(picker.left() >= title.left() && picker.right() <= title.right());
                    assert!((picker.center().y - title.center().y).abs() < px(1.));
                    assert!(title.size.height - picker.size.height < px(12.));
                }
            }
        }
    }
}

#[gpui::test]
fn feature_dialogs(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    open(&shell, &mut cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [760., 1280.] {
            let handle = cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(width), px(820.)));
            for (section, action) in [
                (Section::Providers, "settings-add"),
                (Section::Terminal, "environment-add"),
                (Section::Skills, "skills-install"),
                (Section::Plugins, "plugins-install"),
                (Section::Appearance, "theme-import"),
            ] {
                cx.update(|_, cx| {
                    shell
                        .read(cx)
                        .settings
                        .clone()
                        .update(cx, |settings, cx| settings.select(section, cx))
                });
                draw(&mut cx);
                let button = cx.debug_bounds(action).unwrap();
                cx.simulate_click(button.center(), Modifiers::default());
                draw(&mut cx);
                if section == Section::Appearance {
                    assert!(cx.did_prompt_for_paths());
                    cx.simulate_path_prompt_response(|_| None);
                    draw(&mut cx);
                    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
                    continue;
                }
                assert!(
                    cx.update(|window, cx| window.has_active_dialog(cx)),
                    "{section:?}"
                );
                cx.simulate_keystrokes("escape");
                draw(&mut cx);
                assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
            }
        }
    }
}

#[gpui::test]
fn plugin_navigation(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    open(&shell, &mut cx);
    cx.update(|_, cx| {
        shell
            .read(cx)
            .settings
            .clone()
            .update(cx, |settings, cx| settings.select(Section::Mcp, cx))
    });
    draw(&mut cx);
    let manage = cx.debug_bounds("mcp-open-plugins").unwrap();
    cx.simulate_click(manage.center(), Modifiers::default());
    draw(&mut cx);
    assert!(cx.debug_bounds("plugins-install").is_some());
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).settings.read(cx).section),
        Section::Plugins
    );
}

#[gpui::test]
fn provider_families_and_model_update(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    open(&shell, &mut cx);
    cx.update(|_, cx| {
        shell
            .read(cx)
            .settings
            .clone()
            .update(cx, |settings, cx| settings.select(Section::Providers, cx))
    });
    draw(&mut cx);
    for index in [1, 2, 3, 0] {
        let picker = cx.debug_bounds("provider-families").unwrap();
        let key = crate::settings::ModelCategory::ALL[index].key();
        let tab = cx.debug_bounds(key).unwrap();
        cx.simulate_click(tab.center(), Modifiers::default());
        draw(&mut cx);
        assert_eq!(cx.debug_bounds("provider-0").is_some(), index == 0);
        let tab = cx.debug_bounds(key).unwrap();
        assert!(
            tab.left() >= picker.left() && tab.right() <= picker.right(),
            "selected tab is clipped: {key}"
        );
    }
    let edit = cx.debug_bounds("provider-edit-0").unwrap();
    assert_eq!(edit.size.width, edit.size.height);
    assert!(edit.size.height < px(32.));
    cx.simulate_click(edit.center(), Modifiers::default());
    draw(&mut cx);
    // Settle the newly mounted dialog before hit testing its model toolbar.
    draw(&mut cx);
    let update = cx.debug_bounds("provider-discover").unwrap();
    cx.simulate_click(update.center(), Modifiers::default());
    draw(&mut cx);
    assert!(
        cx.debug_bounds("model-expand-1").is_some(),
        "update: {update:?}, editor: {:?}, model: {:?}, save: {:?}",
        cx.debug_bounds("provider-editor"),
        cx.debug_bounds("model-expand-0"),
        cx.debug_bounds("provider-save")
    );
    let model = cx.debug_bounds("model-expand-0").unwrap();
    cx.simulate_click(model.center(), Modifiers::default());
    draw(&mut cx);
    let save = cx.debug_bounds("provider-save").unwrap();
    cx.simulate_click(save.center(), Modifiers::default());
    draw(&mut cx);
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
}

#[gpui::test]
fn provider_delete_icon(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    open(&shell, &mut cx);
    cx.update(|_, cx| {
        shell.read(cx).settings.clone().update(cx, |settings, cx| {
            settings.select(Section::Providers, cx);
        });
    });
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [760., 1280.] {
            let handle = cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(width), px(820.)));
            draw(&mut cx);
            let edit = cx.debug_bounds("provider-edit-0").unwrap();
            let delete = cx.debug_bounds("provider-delete-0").unwrap();
            assert_eq!(edit.size, delete.size);
            assert_eq!(delete.size.width, delete.size.height);
            assert!(delete.size.height < px(32.));
            cx.simulate_click(delete.center(), Modifiers::default());
            draw(&mut cx);
            assert!(cx.has_pending_prompt());
            crate::prompts::tests::answer(&mut cx, "settings_cancel");
            draw(&mut cx);
            assert!(cx.debug_bounds("provider-0").is_some());
        }
    }
    let delete = cx.debug_bounds("provider-delete-0").unwrap();
    cx.simulate_click(delete.center(), Modifiers::default());
    draw(&mut cx);
    crate::prompts::tests::answer(&mut cx, "settings_delete");
    assert!(!cx.has_pending_prompt());
    assert!(cx.debug_bounds("provider-0").is_none());
}

#[gpui::test]
fn scroll_regions_preserve_feature_rail(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let handle = cx.update(|window, _| window.window_handle());
    cx.simulate_window_resize(handle, size(px(760.), px(560.)));
    open(&shell, &mut cx);
    let conversation = cx.debug_bounds("navigation-conversation").unwrap();
    let heading = cx.debug_bounds("settings-heading").unwrap();
    let body = cx.debug_bounds("settings-content").unwrap();
    cx.simulate_event(ScrollWheelEvent {
        position: body.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-300.))),
        ..Default::default()
    });
    draw(&mut cx);
    assert!(cx.debug_bounds("settings-heading").unwrap().top() < heading.top());
    assert_eq!(
        cx.debug_bounds("navigation-conversation").unwrap(),
        conversation
    );
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(100.), px(400.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-300.))),
        ..Default::default()
    });
    draw(&mut cx);
    assert_eq!(
        cx.debug_bounds("navigation-conversation").unwrap(),
        conversation
    );
}

#[gpui::test]
fn editor_is_a_preview(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    open(&shell, &mut cx);
    let providers = cx.debug_bounds("settings_providers").unwrap();
    cx.simulate_click(providers.center(), Modifiers::default());
    draw(&mut cx);
    let add = cx.debug_bounds("settings-add").unwrap();
    cx.simulate_click(add.center(), Modifiers::default());
    draw(&mut cx);
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    cx.simulate_keystrokes("escape");
    draw(&mut cx);
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
    assert!(cx.debug_bounds("provider-0").is_some());
    assert!(cx.debug_bounds("provider-1").is_none());
}

#[gpui::test]
fn grouped_rows(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    open(&shell, &mut cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark, ThemeMode::Light] {
        for width in [760., 1280.] {
            let handle = cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(width), px(820.)));
            draw(&mut cx);
            cx.update(|_, cx| {
                let theme = cx.theme();
                assert_eq!(
                    theme.background.l,
                    if mode == ThemeMode::Light {
                        1.
                    } else {
                        24. / 255.
                    }
                );
                for surface in [theme.muted, theme.input, theme.border] {
                    assert!(surface.a > 0. && surface.a < 1.);
                }
                assert_eq!(
                    theme.group_box.l,
                    if mode == ThemeMode::Light { 0. } else { 1. }
                );
                if mode == ThemeMode::Light {
                    assert_eq!(theme.group_box.a, 0.);
                } else {
                    assert!(theme.group_box.a > 0. && theme.group_box.a < 1.);
                }
                assert!(theme.popover.a > 0.9);
            });
            let group = cx
                .debug_bounds("settings-group-settings_notifications")
                .unwrap();
            let label = cx
                .debug_bounds("settings-label-settings_notify_system")
                .unwrap();
            let description = cx
                .debug_bounds("settings-description-settings_notify_system")
                .unwrap();
            let toggle = cx
                .debug_bounds("settings-control-settings_notify_system")
                .unwrap();
            assert_eq!(label.left(), description.left());
            assert!(description.top() >= label.bottom());
            assert!(toggle.left() >= label.right());
            assert_eq!(toggle.size.width, px(36.));
            assert!(toggle.size.height >= px(20.));
            assert!((toggle.right() - group.right()).abs() < px(1.));
            assert_eq!(label.size.height, description.size.height);
            assert!(
                cx.debug_bounds("settings-divider-settings_notifications-0")
                    .is_none()
            );
            for selector in [
                "settings-divider-settings_notifications-1",
                "settings-divider-settings_notifications-2",
                "settings-divider-settings_notifications-3",
                "settings-divider-settings_notifications-4",
            ] {
                let divider = cx.debug_bounds(selector).unwrap();
                assert_eq!(divider.size.height, px(1.));
                assert_eq!(divider.left(), group.left());
                assert_eq!(divider.right(), group.right());
            }
            assert!(cx.debug_bounds("settings-group-settings_pairing").is_none());
            cx.update(|_, cx| {
                shell.read(cx).settings.clone().update(cx, |settings, cx| {
                    settings.select(Section::Connections, cx);
                });
            });
            draw(&mut cx);
            let actions = cx
                .debug_bounds("settings-actions-connections_hosts")
                .unwrap();
            let add = cx.debug_bounds("pairing-add").unwrap();
            let share = cx.debug_bounds("pairing-share").unwrap();
            assert!(share.left() >= actions.left());
            assert!(share.right() < add.left());
            assert!(add.right() <= actions.right());
            assert!(
                cx.debug_bounds("settings-group-connections_mobile")
                    .is_some()
            );
            cx.update(|_, cx| {
                shell.read(cx).settings.clone().update(cx, |settings, cx| {
                    settings.select(Section::General, cx);
                });
            });
        }
    }
}

#[gpui::test]
fn provider_dialog_scrolling(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    open(&shell, &mut cx);
    cx.update(|_, cx| {
        shell.read(cx).settings.clone().update(cx, |settings, cx| {
            settings.select(Section::Providers, cx);
        });
    });
    draw(&mut cx);
    let edit = cx.debug_bounds("provider-edit-0").unwrap();
    cx.simulate_click(edit.center(), Modifiers::none());
    draw(&mut cx);
    std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
    for height in [560., 820., 1100.] {
        let handle = cx.update(|window, _| window.window_handle());
        cx.simulate_window_resize(handle, size(px(1280.), px(height)));
        draw(&mut cx);
        let title = cx.debug_bounds("provider-dialog-title").unwrap();
        let body = cx.debug_bounds("provider-editor").unwrap();
        let save = cx.debug_bounds("provider-save").unwrap();
        assert!(title.top() > px(24.));
        assert!(save.bottom() < px(height - 24.));
        assert!(body.top() >= title.bottom());
        assert!(body.bottom() <= save.top());
        let initial = body;
        let toolbar = cx.debug_bounds("provider-discover").unwrap();
        cx.simulate_event(ScrollWheelEvent {
            position: body.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-1000.))),
            ..Default::default()
        });
        draw(&mut cx);
        assert_eq!(cx.debug_bounds("provider-save").unwrap(), save);
        assert_eq!(cx.debug_bounds("provider-editor").unwrap(), initial);
        if height == 560. {
            assert!(cx.debug_bounds("provider-discover").unwrap().top() < toolbar.top());
        }
    }
}
