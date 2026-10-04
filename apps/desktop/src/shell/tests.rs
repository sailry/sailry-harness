use super::*;
use core::prelude::v1::test;

mod approvals;
mod cli;
mod closing;
mod conversation;
mod directory;
mod host;
mod interactions;
mod layout;
mod metrics;
mod project_removal;
mod projects;
mod queue;
mod references;
mod removal;
mod repository;
mod resources;
mod runtime;
mod settings;
mod shortcuts;
mod subagents;
mod tunnels;
mod turns;
mod updates;
mod workspace;
mod worktrees;

pub(crate) fn setup(cx: &mut TestAppContext) -> (Entity<Shell>, VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        init(cx);
    });
    rust_i18n::set_locale("en");
    let mut shell = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut shell = Shell::new(window, cx);
            shell.session = 0;
            shell.navigate(Page::Conversation, window, cx);
            shell
        });
        shell = Some(view.clone());
        Root::new(view, window, cx)
    });
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
    visual.run_until_parked();
    visual.update(|window, cx| {
        _ = window.draw(cx);
    });
    (shell.unwrap(), visual.clone())
}

#[gpui::test]
fn draft_isolation(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.update(cx, |this, cx| {
            this.conversations[&(0, 0)]
                .input
                .update(cx, |input, cx| input.set_value("draft A", window, cx));
            this.navigate(Page::Files, window, cx);
            this.navigate(Page::Conversation, window, cx);
            assert_eq!(
                this.conversations[&(0, 0)].input.read(cx).value(),
                "draft A"
            );
            this.session = 1;
            this.navigate(Page::Conversation, window, cx);
            assert_eq!(this.conversations[&(0, 1)].input.read(cx).value(), "");
            this.select_host(1, window, cx);
            assert_eq!(this.conversations[&(1, 0)].input.read(cx).value(), "");
            this.select_host(0, window, cx);
            assert_eq!(
                this.conversations[&(0, 0)].input.read(cx).value(),
                "draft A"
            );
        })
    });
}

#[gpui::test]
fn search_navigation(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.simulate_keystrokes("secondary-k");
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    cx.simulate_input("Git");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| shell.read(cx).page), Page::Git);
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
}

#[gpui::test]
fn escape_restores_focus(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let input = cx.update(|window, cx| {
        let input = shell.read(cx).conversations[&(0, 0)].input.clone();
        input.update(cx, |input, cx| input.focus(window, cx));
        input
    });
    cx.simulate_keystrokes("secondary-k escape");
    cx.run_until_parked();
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
    cx.simulate_input("restored draft");
    assert_eq!(cx.update(|_, cx| input.read(cx).value()), "restored draft");
}

#[gpui::test]
fn themes_and_sizes(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [1280., 760., 1280.] {
            let handle = cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                assert!(!cx.theme().list.active_highlight);
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(width), px(620.)));
            for page in Page::ALL {
                cx.update(|window, cx| {
                    shell.update(cx, |this, cx| this.navigate(page, window, cx))
                });
                cx.run_until_parked();
                cx.update(|window, cx| {
                    _ = window.draw(cx);
                });
            }
        }
    }
}

mod sidebar {
    use super::*;
    use std::time::Duration;

    fn frame(cx: &mut VisualTestContext, elapsed: u64) -> Bounds<Pixels> {
        cx.executor().advance_clock(Duration::from_millis(elapsed));
        cx.update(|window, cx| {
            window.refresh();
            _ = window.draw(cx);
        });
        cx.debug_bounds("main-content").unwrap()
    }

    #[gpui::test]
    fn slide_and_reverse(cx: &mut TestAppContext) {
        let (_, mut cx) = setup(cx);
        let expanded = frame(&mut cx, 0);
        assert_eq!(expanded.left(), px(NAV_WIDTH + RAIL_WIDTH));
        let toggle = cx.debug_bounds("header-sidebar-toggle").unwrap();
        let title = cx.debug_bounds("resource-title").unwrap();
        let header = cx.debug_bounds("shell-module-header").unwrap();
        assert!(title.left() >= toggle.right());
        assert!(title.top() >= header.top() && title.bottom() <= header.bottom());
        assert!((title.center().y - toggle.center().y).abs() <= px(1.));
        assert!(title.size.height < px(HEADER_HEIGHT));
        cx.simulate_click(toggle.center(), Modifiers::default());
        frame(&mut cx, 0);
        let closing = frame(&mut cx, 50);
        assert!(closing.left() > px(0.) && closing.left() < expanded.left());
        assert!((closing.right() - expanded.right()).abs() < px(1.));
        cx.simulate_keystrokes("secondary-b");
        frame(&mut cx, 0);
        let reopened = frame(&mut cx, 400);
        assert!((reopened.left() - expanded.left()).abs() < px(1.));
        cx.simulate_keystrokes("secondary-b");
        frame(&mut cx, 0);
        let collapsed = frame(&mut cx, 400);
        assert_eq!(collapsed.left(), px(RAIL_WIDTH));
        assert!((collapsed.right() - expanded.right()).abs() < px(1.));
        assert!(cx.debug_bounds("sidebar-settings").is_some());
        cx.simulate_keystrokes("secondary-b");
        frame(&mut cx, 0);
        let opening = frame(&mut cx, 50);
        assert!(opening.left() > px(0.) && opening.left() < expanded.left());
        let restored = frame(&mut cx, 400);
        assert!((restored.left() - expanded.left()).abs() < px(1.));
    }

    #[gpui::test]
    fn preserves_resized_width(cx: &mut TestAppContext) {
        let (shell, mut cx) = setup(cx);
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.navigate(Page::Conversation, window, cx)
            })
        });
        frame(&mut cx, 0);
        cx.update(|window, cx| {
            let panels = shell.read(cx).panels.clone();
            panels.update(cx, |panels, cx| {
                panels.resize_panel(0, px(NAV_WIDTH + RAIL_WIDTH), window, cx);
                panels.resize_panel(0, px(300. + RAIL_WIDTH), window, cx);
            });
            shell.update(cx, |shell, _| shell.layout.sidebar_width = 300.);
        });
        let expanded = frame(&mut cx, 0);
        assert_eq!(expanded.left(), px(300. + RAIL_WIDTH));
        for _ in 0..2 {
            cx.simulate_keystrokes("secondary-b");
            frame(&mut cx, 0);
            frame(&mut cx, 50);
            frame(&mut cx, 400);
        }
        let restored = frame(&mut cx, 0);
        assert!((restored.left() - expanded.left()).abs() < px(1.));
    }

    #[gpui::test]
    fn layout(cx: &mut TestAppContext) {
        let (_, mut cx) = setup(cx);
        for (width, height) in [(1280., 820.), (760., 560.)] {
            let handle = cx.update(|window, _| window.window_handle());
            cx.simulate_window_resize(handle, size(px(width), px(height)));
            cx.run_until_parked();
            cx.update(|window, cx| {
                _ = window.draw(cx);
            });
            assert!(cx.debug_bounds("sidebar-collapse").is_none());
            let first_row = cx.debug_bounds("navigation-conversation").unwrap();
            let second_row = cx.debug_bounds("navigation-activity").unwrap();
            assert!(cx.debug_bounds("sidebar-brand").is_none());
            let first_action = cx.debug_bounds("new-conversation").unwrap();
            let search = cx.debug_bounds("sidebar-search").unwrap();
            let footer = cx.debug_bounds("sidebar-footer").unwrap();
            let settings = cx.debug_bounds("sidebar-settings").unwrap();
            let cpu = cx.debug_bounds("cpu-preview").unwrap();
            let memory = cx.debug_bounds("memory-preview").unwrap();
            assert_eq!(search.left(), first_row.left());
            assert!(first_action.left() >= px(RAIL_WIDTH));
            assert!(second_row.top() - first_row.bottom() <= px(8.));
            assert!(first_action.top() >= px(HEADER_HEIGHT + 16.));
            assert!(first_action.top() <= px(HEADER_HEIGHT + 17.));
            assert!(search.right() < first_action.left());
            assert!(footer.origin.y > px(height - 100.));
            assert!(px(height) - footer.origin.y <= px(48.));
            assert!(footer.bottom() <= px(height));
            assert!(settings.origin.x < px(NAV_WIDTH / 2.));
            assert!(cpu.origin.x > settings.right());
            assert!(memory.origin.x > cpu.origin.x);
            assert!(memory.right() <= px(NAV_WIDTH + RAIL_WIDTH));
            assert!((cpu.center().y - footer.center().y).abs() < px(1.));
            assert_eq!(cpu.size.width, px(64.));
            assert!(cpu.size.height < footer.size.height);
            assert_eq!(memory.size, cpu.size);
            for selector in ["cpu-preview-value", "memory-preview-value"] {
                let label = cx.debug_bounds(selector).unwrap();
                assert!(label.size.height <= cpu.size.height);
                assert!(label.size.width < cpu.size.width);
            }
        }
    }

    #[gpui::test]
    fn host_rows(cx: &mut TestAppContext) {
        let (shell, mut cx) = setup(cx);
        for (host, row, name, detail) in [
            (1, "host-1", "host-name-1", "host-detail-1"),
            (0, "host-0", "host-name-0", "host-detail-0"),
        ] {
            let row = cx.debug_bounds(row).unwrap();
            let name = cx.debug_bounds(name).unwrap();
            assert!(cx.debug_bounds(detail).is_none());
            assert_eq!(name.center().y, row.center().y);
            assert!(row.size.height <= px(32.));
            assert!(name.right() <= row.right());
            cx.simulate_click(row.center(), Modifiers::default());
            cx.run_until_parked();
            assert_eq!(cx.update(|_, cx| shell.read(cx).host), host);
        }
    }

    #[gpui::test]
    fn session_hover_and_expansion(cx: &mut TestAppContext) {
        let (_, mut cx) = setup(cx);
        assert!(cx.debug_bounds("session-worktree-0").is_none());
        for (selector, icon_selector) in [
            ("terminal-0", "terminal-icon-0"),
            ("session-1", "session-worktree-1"),
        ] {
            let row = cx.debug_bounds(selector).unwrap();
            let icon = cx.debug_bounds(icon_selector).unwrap();
            assert!(icon.left() > row.center().x);
            assert!(icon.right() <= row.right());
            assert!(icon.bottom() <= row.bottom());
            cx.simulate_mouse_move(row.center(), None, Modifiers::default());
            frame(&mut cx, 0);
            assert_eq!(cx.debug_bounds(icon_selector), Some(icon));
            cx.simulate_mouse_move(point(px(500.), px(100.)), None, Modifiers::default());
            frame(&mut cx, 0);
            assert_eq!(cx.debug_bounds(icon_selector), Some(icon));
        }
        let project = cx.debug_bounds("sidebar-project").unwrap();
        let title = project.origin + point(px(65.), project.size.height / 2.);
        cx.simulate_click(title, Modifiers::default());
        frame(&mut cx, 400);
        assert!(cx.debug_bounds("session-0").is_none());
        cx.simulate_click(title, Modifiers::default());
        frame(&mut cx, 400);
        assert!(cx.debug_bounds("session-0").is_some());
    }

    #[gpui::test]
    fn selected_hover_contrast(cx: &mut TestAppContext) {
        let (_, mut cx) = setup(cx);
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                let normal = crate::theme::sidebar_item(false, false, cx);
                let hovered = crate::theme::sidebar_item(false, true, cx);
                let selected = crate::theme::sidebar_item(true, false, cx);
                let selected_hovered = crate::theme::sidebar_item(true, true, cx);
                assert_eq!(normal.a, 0.);
                if mode.is_dark() {
                    assert!(hovered.a < selected.a);
                    assert!(selected_hovered.a > hovered.a);
                } else {
                    assert!(selected.a < selected_hovered.a);
                    assert!(selected_hovered.a < hovered.a);
                    assert_eq!(hovered, cx.theme().accent);
                    assert_eq!(selected, cx.theme().secondary_active);
                }
                assert!(selected.a < selected_hovered.a);
                assert_eq!(selected.h, cx.theme().sidebar_foreground.h);
                assert_eq!(selected.l, cx.theme().sidebar_foreground.l);
            });
        }
    }

    #[gpui::test]
    fn indicators_are_read_only(cx: &mut TestAppContext) {
        let (shell, mut cx) = setup(cx);
        for selector in ["cpu-preview", "memory-preview", "sidebar-footer"] {
            let bounds = cx.debug_bounds(selector).unwrap();
            cx.simulate_click(bounds.center(), Modifiers::default());
            cx.run_until_parked();
            assert_eq!(cx.update(|_, cx| shell.read(cx).page), Page::Conversation);
            assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
        }
    }

    #[gpui::test]
    fn controls(cx: &mut TestAppContext) {
        let (shell, mut cx) = setup(cx);
        let settings = cx.debug_bounds("sidebar-settings").unwrap();
        cx.simulate_click(settings.center(), Modifiers::default());
        cx.run_until_parked();
        assert_eq!(cx.update(|_, cx| shell.read(cx).page), Page::Settings);
        assert!(cx.debug_bounds("settings-back").is_none());
        let conversation = cx.debug_bounds("navigation-conversation").unwrap();
        cx.simulate_click(conversation.center(), Modifiers::default());
        cx.run_until_parked();
        let search = cx.debug_bounds("sidebar-search").unwrap();
        cx.simulate_click(search.center(), Modifiers::default());
        cx.run_until_parked();
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        let collapse = cx.debug_bounds("header-sidebar-toggle").unwrap();
        cx.simulate_click(collapse.center(), Modifiers::default());
        cx.run_until_parked();
        assert!(!cx.update(|_, cx| shell.read(cx).layout.sidebar_open));
        cx.simulate_keystrokes("secondary-b");
        cx.run_until_parked();
        assert!(cx.update(|_, cx| shell.read(cx).layout.sidebar_open));
    }
}

#[gpui::test]
fn header_panel_actions(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    for page in Page::ALL.into_iter().chain([Page::Plugin]) {
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| shell.navigate(page, window, cx));
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let module = cx.debug_bounds("shell-module-header").unwrap();
        let inner = cx.debug_bounds("workspace-header");
        let actions = cx.debug_bounds("header-actions").unwrap();
        assert_eq!(module.top(), px(0.));
        assert_eq!(
            cx.debug_bounds("shell-body").unwrap().top(),
            module.bottom()
        );
        assert!(inner.is_none(), "{page:?} must use the outer header only");
        assert!(actions.top() >= module.top() && actions.bottom() <= module.bottom());
        assert!(actions.right() <= module.right());
        assert_eq!(
            cx.debug_bounds("header-more").is_some(),
            matches!(page, Page::Conversation | Page::Project),
            "unexpected header menu for {page:?}"
        );
    }
}
