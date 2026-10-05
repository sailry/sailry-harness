use super::*;

fn frame(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(400));
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
}

#[gpui::test]
fn launcher_has_filled_controls_in_both_themes(cx: &mut TestAppContext) {
    let (_shell, mut visual) = setup(cx);
    super::workspace::click(&mut visual, "toggle-details");
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        visual.update(|window, cx| {
            Theme::change(mode, Some(window), cx);
            cx.refresh_windows();
        });
        frame(&mut visual);
        for selector in [
            "launch-resource_browser",
            "launch-terminal",
            "launch-files",
            "launch-resource_review",
        ] {
            let bounds = visual.debug_bounds(selector).unwrap();
            assert_eq!(bounds.size.height, px(40.));
            visual.update(|window, cx| {
                let bounds = bounds.scale(window.scale_factor());
                let expected = cx.theme().secondary;
                assert!(expected.a > 0.);
                assert!(
                    window.painted_quads().into_iter().any(|quad| {
                        quad.bounds == bounds && quad.background == expected.into()
                    }),
                    "{selector} must retain a semantic surface in {mode:?}"
                );
            });
            visual.simulate_mouse_move(bounds.center(), None, Modifiers::default());
            frame(&mut visual);
            visual.update(|window, cx| {
                let bounds = bounds.scale(window.scale_factor());
                assert!(
                    window.painted_quads().into_iter().any(|quad| {
                        quad.bounds == bounds
                            && quad.background == cx.theme().button_secondary_hover.into()
                    }),
                    "{selector} must retain the Kit hover state"
                );
            });
            visual.simulate_mouse_move(point(px(0.), px(0.)), None, Modifiers::default());
            frame(&mut visual);
        }
    }
    visual.update(|window, _| window.remove_window());
}

fn assert_split(cx: &mut VisualTestContext, page: Page) {
    let editor = cx
        .debug_bounds(if page == Page::Git {
            "git-diff"
        } else {
            "file-preview"
        })
        .unwrap();
    let list = cx
        .debug_bounds(if page == Page::Git {
            "git-changes"
        } else {
            "file-explorer"
        })
        .unwrap();
    assert!(editor.right() <= list.left());
    assert!(
        editor.size.width >= px(240.),
        "{page:?}: editor={editor:?}, list={list:?}"
    );
    assert!(list.size.width >= px(180.));
    let header = cx.debug_bounds("document-header").unwrap();
    if cx.debug_bounds("resource-side-panel").is_none() {
        let outer = cx.debug_bounds("shell-module-header").unwrap();
        assert_eq!(header.top(), outer.top());
        assert_eq!(header.bottom(), outer.bottom());
        assert!(cx.debug_bounds("workspace-header").is_none());
    }
    let content = cx.debug_bounds("document-editor").unwrap();
    assert!(header.bottom() <= content.top());
    let footer = cx.debug_bounds("document-path").unwrap();
    assert_eq!(footer.size.height, px(32.));
    assert!((footer.bottom() - editor.bottom()).abs() <= px(1.));
    assert!(content.bottom() <= footer.top());
    assert!(cx.debug_bounds("document-tab-0-icon").is_none());
    if page == Page::Files {
        let toolbar = cx.debug_bounds("file-toolbar").unwrap();
        let save = cx.debug_bounds("save-file").unwrap();
        let undo = cx.debug_bounds("files_undo").unwrap();
        let mode = cx.debug_bounds("markdown-source-mode").unwrap();
        assert_eq!(toolbar.size.height, px(48.));
        assert!(save.right() < undo.left());
        assert!(mode.left() > undo.right());
        assert!(mode.right() <= toolbar.right());
        assert!((save.center().y - mode.center().y).abs() <= px(1.));
        assert_eq!(toolbar.bottom(), content.top());
    } else {
        assert!(cx.debug_bounds("document-diff-count").is_some());
    }

    let tab = cx.debug_bounds("document-tab-0").unwrap();
    assert_eq!(tab.size.height, px(28.));
    // A bottom border can inset the content center by half a pixel.
    assert!((tab.center().y - header.center().y).abs() <= px(0.5));
    assert!(cx.debug_bounds("resource-tab-files").is_none());
    assert!(cx.debug_bounds("resource-tab-git").is_none());
    if page == Page::Git {
        let commit = cx.debug_bounds("git-commit-controls").unwrap();
        assert!(commit.left() >= list.left());
        assert!(commit.right() <= list.right());
    }
}

#[gpui::test]
fn closing_preview_tabs_keeps_the_successor_focused(cx: &mut TestAppContext) {
    let (shell, mut visual) = setup(cx);
    for page in [Page::Files, Page::Git] {
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.navigate(Page::Conversation, window, cx);
                shell.open_resource_panel(page, window, cx);
            })
        });
        frame(&mut visual);
        // Capture the first content before opening another tab scrolls a narrow
        // strip. Debug bounds describe the entire tab, including clipped parts.
        super::workspace::click(&mut visual, "document-tab-0");
        frame(&mut visual);
        let first = visual.update(|window, cx| window.focused(cx).unwrap());
        super::workspace::click(
            &mut visual,
            if page == Page::Git {
                "resource-change-1"
            } else {
                "resource-file-1"
            },
        );
        super::workspace::click(&mut visual, "document-tab-1");
        frame(&mut visual);
        let second = visual.update(|window, cx| window.focused(cx).unwrap());
        assert_ne!(first, second);
        assert_eq!(
            shell.read_with(&visual, |shell, _| if page == Page::Git {
                shell.git_state(true).tabs.selected
            } else {
                shell.file_state(true).tabs.selected
            }),
            1
        );
        visual.simulate_keystrokes("secondary-w");
        frame(&mut visual);
        assert!(visual.debug_bounds("document-tab-1").is_none());
        assert!(visual.debug_bounds("document-tab-0").is_some());
        visual.update(|window, cx| {
            let tabs=if page==Page::Git {&shell.read(cx).git_state(true).tabs} else {&shell.read(cx).file_state(true).tabs};
            assert!(first.is_focused(window), "preview {page:?} successor focus: expected={first:?}, current={:?}, selected={}, open={:?}",window.focused(cx),tabs.selected,tabs.open);
        });
        visual.simulate_keystrokes("secondary-w");
        frame(&mut visual);
        assert!(visual.debug_bounds("document-tab-0").is_none());
        if page == Page::Git {
            assert!(visual.debug_bounds("resource-side-panel").is_some());
            visual.simulate_keystrokes("secondary-w");
            frame(&mut visual);
        }
        assert!(visual.debug_bounds("resource-side-panel").is_none());
        assert!(visual.update(|_, cx| shell.read(cx).side_resource.is_none()));
    }
}

#[gpui::test]
fn shared_resource_layout(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let handle = cx.update(|window, _| window.window_handle());
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for page in [Page::Files, Page::Git] {
            cx.simulate_window_resize(handle, size(px(1440.), px(820.)));
            cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                shell.update(cx, |shell, cx| {
                    shell.navigate(Page::Conversation, window, cx);
                    shell.open_resource_panel(page, window, cx);
                });
            });
            frame(&mut cx);
            assert_split(&mut cx, page);
            let conversation = cx.debug_bounds("conversation-page").unwrap();
            let panel = cx.debug_bounds("resource-side-panel").unwrap();
            assert!(conversation.right() <= panel.left());
            let header = cx.debug_bounds("document-header").unwrap();
            let controls = cx
                .debug_bounds(if page == Page::Git {
                    "git-changes-header"
                } else {
                    "file-explorer-header"
                })
                .unwrap();
            assert_eq!(header.top(), panel.top());
            assert_eq!(controls.top(), panel.top());
            assert!(cx.debug_bounds("close-details").is_none());
            assert!(cx.debug_bounds("toggle-details").unwrap().right() <= conversation.right());
            assert!(cx.debug_bounds("document-tab-0").unwrap().size.width <= px(180.));
            assert!(cx.debug_bounds("document-path").is_some());
            cx.update(|_, cx| {
                let tabs = if page == Page::Git {
                    &shell.read(cx).git.tabs
                } else {
                    &shell.read(cx).files.tabs
                };
                assert_eq!(
                    tabs.label(tabs.selected).as_ref(),
                    tabs.names[tabs.selected].rsplit('/').next().unwrap()
                );
            });
            let file = cx
                .debug_bounds(if page == Page::Git {
                    "resource-change-1"
                } else {
                    "resource-file-1"
                })
                .unwrap();
            cx.simulate_click(file.center(), Modifiers::default());
            frame(&mut cx);
            assert!(cx.debug_bounds("document-tab-1").is_some());
            let close_document = cx.debug_bounds("close-document-1").unwrap();
            cx.simulate_click(close_document.center(), Modifiers::default());
            frame(&mut cx);
            assert!(cx.debug_bounds("document-tab-1").is_none());
            assert!(cx.debug_bounds("document-tab-0").is_some());
            assert_eq!(cx.update(|_, cx| shell.read(cx).page), Page::Conversation);
            let close = cx.debug_bounds("toggle-details").unwrap();
            cx.simulate_click(close.center(), Modifiers::default());
            frame(&mut cx);
            assert!(cx.debug_bounds("resource-side-panel").is_none());
            assert!(cx.update(|_, cx| shell.read(cx).side_resource.is_none()));
            cx.update(|window, cx| shell.update(cx, |shell, cx| shell.navigate(page, window, cx)));
            frame(&mut cx);
            assert_split(&mut cx, page);
            assert!(cx.debug_bounds("resource-side-panel").is_none());
            let main = cx.debug_bounds("main-content").unwrap();
            let list = cx
                .debug_bounds(if page == Page::Git {
                    "git-changes"
                } else {
                    "file-explorer"
                })
                .unwrap();
            assert!(main.right() <= list.left());
            assert_eq!(main.left(), px(RAIL_WIDTH));
            assert!(cx.debug_bounds("shell-navigation").is_none());
            let handle = cx.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    shell.layout.sidebar_open = false;
                    shell.files.tabs.open(1);
                    shell.git.tabs.open(1);
                    cx.notify();
                });
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(760.), px(820.)));
            frame(&mut cx);
            let main = cx.debug_bounds("main-content").unwrap();
            let toggle = cx.debug_bounds("toggle-details").unwrap();
            assert!(toggle.right() <= main.right());
            assert!(cx.debug_bounds("shell-feature-rail").is_some());
            assert!(cx.debug_bounds("shell-navigation").is_none());
            cx.simulate_window_resize(handle, size(px(1000.), px(820.)));
            frame(&mut cx);
            assert_split(&mut cx, page);
            super::workspace::click(&mut cx, "toggle-details");
            assert!(
                cx.debug_bounds(if page == Page::Git {
                    "git-changes"
                } else {
                    "file-explorer"
                })
                .is_none()
            );
            super::workspace::click(&mut cx, "toggle-details");
            assert_split(&mut cx, page);
            cx.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    shell.layout.sidebar_open = true;
                    shell.navigate(Page::Conversation, window, cx);
                })
            });
            cx.simulate_window_resize(handle, size(px(1280.), px(820.)));
            frame(&mut cx);
        }
    }
}

#[gpui::test]
fn panel_lifetimes_are_independent(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.files.tabs.open(1);
            shell.git.tabs.open(1);
            shell.navigate(Page::Files, window, cx);
            shell.navigate(Page::Git, window, cx);
            shell.navigate(Page::Conversation, window, cx);
            assert_eq!(shell.files.tabs.open, vec![0, 1]);
            assert_eq!(shell.git.tabs.open, vec![0, 1]);
            shell.open_resource_panel(Page::Git, window, cx);
            assert_eq!(shell.git_state(true).tabs.open, vec![0]);
            shell.git_state_mut(true).tabs.open(1);
        })
    });
    frame(&mut cx);
    let old_editor = cx.update(|_, cx| shell.read(cx).git_state(true).tabs.editors[1].downgrade());
    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.close_resource_panel(cx)));
    frame(&mut cx);
    assert!(old_editor.upgrade().is_none());
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_resource_panel(Page::Git, window, cx);
            assert_eq!(shell.git_state(true).tabs.open, vec![0]);
            assert_eq!(shell.git.tabs.open, vec![0, 1]);
            shell.close_resource_panel(cx);
            shell.open_resource_panel(Page::Files, window, cx);
            assert_eq!(shell.file_state(true).tabs.open, vec![0]);
            shell.file_state_mut(true).tabs.open(1);
            shell.close_resource_panel(cx);
            shell.open_resource_panel(Page::Files, window, cx);
            assert_eq!(shell.file_state(true).tabs.open, vec![0]);
            assert_eq!(shell.files.tabs.open, vec![0, 1]);
        })
    });
    frame(&mut cx);
}

#[gpui::test]
fn conversation_links_preserve_drafts(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.conversations[&(0, 0)].input.update(cx, |input, cx| {
                input.set_value("Preserved draft", window, cx)
            });
            shell.open_conversation_link("docs/example.md#layout".into(), window, cx);
            assert_eq!(shell.file_state(true).tabs.selected, 1);
            assert_eq!(shell.files.tabs.selected, 0);
            assert_eq!(shell.page, Page::Conversation);
            assert_eq!(
                shell.conversations[&(0, 0)].input.read(cx).value(),
                "Preserved draft"
            );
        })
    });
    frame(&mut cx);
    assert_split(&mut cx, Page::Files);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_conversation_link("https://example.invalid/preview".into(), window, cx)
        })
    });
    frame(&mut cx);
    assert!(cx.debug_bounds("resource-tool-preview").is_some());
    let handle = cx.update(|window, _| window.window_handle());
    cx.simulate_window_resize(handle, size(px(760.), px(620.)));
    frame(&mut cx);
    assert!(cx.debug_bounds("resource-side-panel").is_none());
    assert!(cx.update(|_, cx| shell.read(cx).layout.panel_open[0]));
    cx.simulate_window_resize(handle, size(px(1280.), px(820.)));
    frame(&mut cx);
    assert!(cx.debug_bounds("resource-tool-preview").is_some());
}

fn drag_divider(cx: &mut VisualTestContext, start: Point<Pixels>, delta: f32) {
    cx.simulate_mouse_move(start, None, Modifiers::default());
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    for step in 1..=4 {
        cx.simulate_mouse_move(
            point(start.x + px(delta * step as f32 / 4.), start.y),
            MouseButton::Left,
            Modifiers::default(),
        );
        frame(cx);
    }
    cx.simulate_mouse_up(
        point(start.x + px(delta), start.y),
        MouseButton::Left,
        Modifiers::default(),
    );
    frame(cx);
}

#[gpui::test]
fn native_resize_handles(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let handle = cx.update(|window, _| window.window_handle());
    cx.simulate_window_resize(handle, size(px(1600.), px(820.)));
    for page in [Page::Files, Page::Git] {
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.navigate(Page::Conversation, window, cx);
                shell.open_resource_panel(page, window, cx);
            });
        });
        frame(&mut cx);
        let before = cx.debug_bounds("resource-side-panel").unwrap();
        drag_divider(&mut cx, point(before.left(), before.center().y), -60.);
        let after = cx.debug_bounds("resource-side-panel").unwrap();
        assert!(after.size.width > before.size.width + px(20.));
        let saved = cx.update(|_, cx| shell.read(cx).layout.panel_width[0]);
        assert!((px(saved) - after.size.width).abs() < px(2.));

        let list_selector = if page == Page::Git {
            "git-changes"
        } else {
            "file-explorer"
        };
        let before = cx.debug_bounds(list_selector).unwrap();
        drag_divider(&mut cx, point(before.left(), before.center().y), -30.);
        let after = cx.debug_bounds(list_selector).unwrap();
        assert!(after.size.width > before.size.width + px(10.));
        assert_split(&mut cx, page);

        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.close_resource_panel(cx);
                shell.navigate(page, window, cx);
            });
        });
        frame(&mut cx);
        let before = cx.debug_bounds(list_selector).unwrap();
        drag_divider(&mut cx, point(before.left(), before.center().y), -30.);
        let after = cx.debug_bounds(list_selector).unwrap();
        assert!(after.size.width > before.size.width + px(10.));
        assert_split(&mut cx, page);
        let saved = cx.update(|_, cx| shell.read(cx).layout.panel_width[page.panel_index()]);
        assert!((px(saved) - after.size.width).abs() < px(2.));
    }
}

#[gpui::test]
fn launcher_context(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        cx.update(|window, cx| Theme::change(mode, Some(window), cx));
        for (entry, expected) in [
            ("launch-resource_review", "git-diff"),
            ("launch-terminal", "resource-tool-preview"),
            ("launch-resource_browser", "resource-tool-preview"),
            ("launch-files", "file-preview"),
        ] {
            frame(&mut cx);
            let toggle = cx.debug_bounds("toggle-details").unwrap();
            cx.simulate_click(toggle.center(), Modifiers::default());
            frame(&mut cx);
            assert!(cx.debug_bounds("resource-launcher").is_some());
            assert!(cx.debug_bounds("document-header").is_none());
            let bounds = cx.debug_bounds(entry).unwrap();
            cx.simulate_click(bounds.center(), Modifiers::default());
            frame(&mut cx);
            assert!(cx.debug_bounds("resource-launcher").is_none());
            assert!(
                cx.debug_bounds(expected).is_some(),
                "{entry}: missing {expected}"
            );
            assert_eq!(cx.update(|_, cx| shell.read(cx).page), Page::Conversation);
            let close = cx.debug_bounds("toggle-details").unwrap();
            cx.simulate_click(close.center(), Modifiers::default());
            frame(&mut cx);
            assert!(cx.update(|_, cx| shell.read(cx).side_resource.is_none()));
        }
        for (destination, expected) in [
            (crate::resources::launcher::Destination::Review, "git-diff"),
            (
                crate::resources::launcher::Destination::Terminal,
                "resource-tool-preview",
            ),
            (
                crate::resources::launcher::Destination::Files,
                "file-preview",
            ),
        ] {
            cx.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    shell.open_destination(destination, window, cx)
                })
            });
            frame(&mut cx);
            assert!(cx.debug_bounds(expected).is_some());
            let close = cx.debug_bounds("toggle-details").unwrap();
            cx.simulate_click(close.center(), Modifiers::default());
            frame(&mut cx);
        }
    }
}

#[gpui::test]
fn first_editor_geometry(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let handle = cx.update(|window, _| window.window_handle());
    for width in [1600., 2048.] {
        cx.simulate_window_resize(handle, size(px(width), px(820.)));
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.open_resource_panel(Page::Files, window, cx);
                shell.file_state_mut(true).tabs.open.clear();
                cx.notify();
            });
        });
        frame(&mut cx);
        let compact = cx.debug_bounds("file-explorer").unwrap();
        let header = cx.debug_bounds("file-explorer-header").unwrap();
        assert_eq!(header.left(), compact.left());
        let panel = cx.debug_bounds("resource-side-panel").unwrap();
        assert_eq!(compact.left(), panel.left());
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.file_state_mut(true).tabs.open(0);
                cx.notify();
            });
            window.draw(cx).clear(cx);
        });
        let first = cx.debug_bounds("file-explorer").unwrap();
        let header = cx.debug_bounds("file-explorer-header").unwrap();
        assert_eq!(header.left(), first.left());
        assert!((first.size.width - compact.size.width).abs() <= px(1.));
        assert_eq!(first.right(), compact.right());
        frame(&mut cx);
        assert_eq!(cx.debug_bounds("file-explorer").unwrap(), first);
    }
}

#[gpui::test]
fn last_diff_collapses_sidebar(cx: &mut TestAppContext) {
    let (shell, mut visual) = setup(cx);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_resource_panel(Page::Git, window, cx)
        });
    });
    frame(&mut visual);
    let expanded = visual.debug_bounds("resource-side-panel").unwrap();
    let close = visual.debug_bounds("close-document-0").unwrap();
    visual.simulate_click(close.center(), Modifiers::default());
    frame(&mut visual);
    assert!(visual.debug_bounds("git-diff").is_none());
    let compact = visual.debug_bounds("resource-side-panel").unwrap();
    let changes = visual.debug_bounds("git-changes").unwrap();
    assert!(compact.size.width < expanded.size.width);
    assert_eq!(changes.left(), compact.left());
    assert_eq!(changes.right(), compact.right());

    let file = visual.debug_bounds("resource-change-0").unwrap();
    visual.simulate_click(file.center(), Modifiers::default());
    frame(&mut visual);
    assert!(visual.debug_bounds("git-diff").is_some());
    assert_eq!(
        visual.debug_bounds("resource-side-panel").unwrap(),
        expanded
    );
    visual.update(|window, cx| {
        shell.read(cx).panel_focus.clone().focus(window, cx);
    });
    visual.simulate_keystrokes("secondary-w");
    frame(&mut visual);
    assert!(visual.debug_bounds("git-diff").is_none());
    assert_eq!(visual.debug_bounds("resource-side-panel").unwrap(), compact);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.navigate(Page::Git, window, cx);
            shell.git.tabs.open.clear();
            cx.notify();
        });
    });
    frame(&mut visual);
    assert!(visual.debug_bounds("git-diff").is_some());
}

#[gpui::test]
fn conversation_panel_geometry(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let handle = cx.update(|window, _| window.window_handle());
    for page in [Page::Files, Page::Git] {
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.navigate(Page::Conversation, window, cx);
                shell.layout.conversation_panel_resized = false;
                shell.open_resource_panel(page, window, cx);
            })
        });
        let list_id = if page == Page::Git {
            "git-changes"
        } else {
            "file-explorer"
        };
        let list_width = if page == Page::Git { 300_f32 } else { 220. };
        for width in [1024., 1600., 2048.] {
            cx.simulate_window_resize(handle, size(px(width), px(820.)));
            frame(&mut cx);
            let panel = cx
                .debug_bounds("resource-side-panel")
                .expect("panel opens when main area fits");
            let nav = RAIL_WIDTH + cx.update(|_, cx| shell.read(cx).layout.sidebar_width);
            assert!(
                (panel.size.width - px((width - nav) / 2.)).abs() <= px(1.),
                "{panel:?}"
            );
            let list = cx.debug_bounds(list_id).unwrap();
            assert!(
                (list.size.width - px(list_width.min((width - nav) / 2. - 180.))).abs() <= px(2.),
                "{list:?}"
            );
        }
        let before = cx.debug_bounds(list_id).unwrap();
        drag_divider(&mut cx, point(before.left(), before.center().y), -35.);
        let preferred = cx.debug_bounds(list_id).unwrap().size.width;
        assert!(preferred > before.size.width + px(10.));
        cx.simulate_window_resize(handle, size(px(1600.), px(820.)));
        frame(&mut cx);
        assert!((cx.debug_bounds(list_id).unwrap().size.width - preferred).abs() <= px(2.));
        let panel = cx.debug_bounds("resource-side-panel").unwrap();
        drag_divider(&mut cx, point(panel.left(), panel.center().y), -50.);
        let saved = cx.debug_bounds("resource-side-panel").unwrap().size.width;
        assert!(saved > panel.size.width + px(20.));
        cx.simulate_window_resize(handle, size(px(2048.), px(820.)));
        frame(&mut cx);
        assert!(
            (cx.debug_bounds("resource-side-panel").unwrap().size.width - saved).abs() <= px(2.)
        );
        assert!((cx.debug_bounds(list_id).unwrap().size.width - preferred).abs() <= px(2.));
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.close_resource_panel(cx);
                shell.open_resource_panel(page, window, cx);
            })
        });
        frame(&mut cx);
        assert!(
            (cx.debug_bounds("resource-side-panel").unwrap().size.width - saved).abs() <= px(2.)
        );
    }
}

#[gpui::test]
fn focused_close_shortcut(cx: &mut TestAppContext) {
    fn source(cx: &mut VisualTestContext) {
        frame(cx);
        let bounds = cx.debug_bounds("markdown-source-mode").unwrap();
        cx.simulate_click(bounds.center(), Modifiers::default());
        frame(cx);
    }

    let (shell, mut visual) = setup(cx);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_resource_panel(Page::Files, window, cx);
            shell.file_state_mut(true).tabs.open(1);
        })
    });
    source(&mut visual);
    visual.simulate_keystrokes("secondary-w");
    frame(&mut visual);
    assert!(shell.read_with(&visual, |shell, _| shell.side_resource.is_some()));
    assert_eq!(
        shell.read_with(&visual, |shell, _| shell.file_state(true).tabs.open.len()),
        1
    );
    source(&mut visual);
    visual.simulate_keystrokes("secondary-w");
    frame(&mut visual);
    assert!(shell.read_with(&visual, |shell, _| shell.side_resource.is_none()));
    assert!(!visual.has_pending_prompt());
}

#[gpui::test]
fn browser_preview(cx: &mut TestAppContext) {
    let (shell, mut visual) = setup(cx);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.navigate(Page::Conversation, window, cx);
            shell.open_conversation_link("https://example.test/page".into(), window, cx);
            assert!(matches!(
                shell.side_resource,
                Some(crate::resources::SideResource::Tool(
                    crate::resources::launcher::Destination::Browser
                ))
            ));
        });
    });
    frame(&mut visual);
    assert!(visual.debug_bounds("resource-tool-preview").is_some());
    assert!(visual.debug_bounds("browser-go").is_none());
}

#[gpui::test]
fn browser_tool_panel(cx: &mut TestAppContext) {
    use sailry_protocol::{
        NodeId, RequestId, SessionId,
        browser::{Action, Call},
    };
    let (shell, mut visual) = setup(cx);
    let node = NodeId([7; 32]);
    let session = SessionId::new();
    for action in [Action::Tabs, Action::Read { tab: None }] {
        let task = visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.session_scope.active = session_scope::Key::Session(node, session);
                shell.sync_session_panel(window, cx);
                shell.layout.panel_open[0] = false;
                shell.side_resource = None;
                shell.execute_browser_call(
                    node,
                    Call {
                        id: RequestId::new(),
                        session,
                        action,
                        expires_at_ms: u64::MAX,
                    },
                    window,
                    cx,
                )
            })
        });
        // Preview fixtures must not execute native browser automation.
        assert!(visual.foreground_executor().block_on(task).is_err());
        frame(&mut visual);
        assert!(visual.debug_bounds("browser-panel").is_none());
    }
    let task = visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.layout.panel_open[0] = false;
            shell.execute_browser_call(
                node,
                Call {
                    id: RequestId::new(),
                    session: SessionId::new(),
                    action: Action::Tabs,
                    expires_at_ms: u64::MAX,
                },
                window,
                cx,
            )
        })
    });
    assert!(visual.foreground_executor().block_on(task).is_err());
    assert!(!shell.read_with(&visual, |shell, _| shell.layout.panel_open[0]));
}
