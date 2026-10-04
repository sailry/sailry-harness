use super::*;
use core::prelude::v1::test;
use gpui_kit::component::Root;
use std::time::Duration;

mod scroll;

fn borderless_fill(visual: &mut VisualTestContext, bounds: Bounds<Pixels>, expected: Hsla) {
    visual.update(|window, _| {
        let bounds = bounds.scale(window.scale_factor());
        let quads: Vec<_> = window
            .painted_quads()
            .into_iter()
            .filter(|quad| quad.bounds == bounds)
            .collect();
        assert!(
            quads
                .iter()
                .all(|quad| quad.border_widths == gpui_kit::Edges::default())
        );
        if expected.a > 0. {
            assert!(quads.iter().any(|quad| quad.background == expected.into()));
        }
    });
}

#[gpui::test]
fn highlights_keep_borderless_fills(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        owner = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = owner.unwrap();
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        visual.update(|window, cx| {
            Theme::change(mode, Some(window), cx);
            shell.update(cx, |shell, cx| {
                shell.page = Page::Host;
                shell.host = 0;
                shell.sidebar.hovered = None;
                cx.notify();
            });
            window.draw(cx).clear(cx);
        });
        for (selector, row, selected, project) in [
            ("host-0", Row::Host(0), true, false),
            ("sidebar-project", Row::Project(0), false, true),
            ("session-0", Row::Session(0, 0), false, false),
        ] {
            let bounds = visual.debug_bounds(selector).unwrap();
            let fill = visual.update(|_, cx| theme::sidebar_item(selected, false, cx));
            borderless_fill(visual, bounds, fill);
            visual.simulate_mouse_move(bounds.center(), None, Modifiers::default());
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert_eq!(visual.debug_bounds(selector), Some(bounds));
            assert!(shell.read_with(visual, |shell, _| shell.sidebar.hovered == Some(row)));
            let fill = visual.update(|_, cx| theme::sidebar_item(selected, !project, cx));
            borderless_fill(visual, bounds, fill);
            visual.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let fill = visual.update(|_, cx| theme::sidebar_item(selected, false, cx));
            borderless_fill(visual, bounds, fill);
        }
        let host = visual.debug_bounds("host-1").unwrap();
        visual.simulate_click(host.center(), Modifiers::default());
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(shell.read_with(visual, |shell, _| shell.host), 1);
        let hovered = shell.read_with(visual, |shell, _| {
            shell.sidebar.hovered == Some(Row::Host(1))
        });
        let fill = visual.update(|_, cx| theme::sidebar_item(true, hovered, cx));
        let bounds = visual.debug_bounds("host-1").unwrap();
        borderless_fill(visual, bounds, fill);
    }
    visual.update(|window, _| window.remove_window());
}

#[gpui::test]
fn empty_projects_ignores_other_hosts(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        owner = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = owner.unwrap();
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            assert!(shell.live.is_none());
            shell.page = Page::Host;
            let host = shell.host;
            shell
                .workspace
                .projects
                .retain(|_, project| project.host != host);
            let projects = &shell.workspace.projects;
            shell
                .workspace
                .worktrees
                .retain(|_, tree| projects.contains_key(&tree.project));
            assert!(!shell.workspace.projects.is_empty());
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
    assert!(visual.debug_bounds("sidebar-projects-empty").is_some());
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.host = shell.workspace.projects.values().next().unwrap().host;
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
    assert!(visual.debug_bounds("sidebar-projects-empty").is_none());
    visual.update(|window, _| window.remove_window());
}

#[gpui::test]
fn preview_locations_stay_inline(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        owner = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = owner.unwrap();
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            assert!(shell.live.is_none());
            shell.sidebar.recent_preview = vec![(0, 2)];
            shell.sidebar.recent_open = true;
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
    for (row, branch) in [
        ("session-2", "session-branch-2"),
        ("recent-preview-2", "recent-preview-2-branch"),
    ] {
        let bounds = visual.debug_bounds(row).unwrap();
        assert!(visual.debug_bounds(branch).is_none());
        visual.simulate_mouse_move(bounds.center(), None, Modifiers::default());
        visual.executor().advance_clock(Duration::from_secs(1));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds(branch).is_some());
        assert!(visual.debug_bounds("tooltip-popup").is_none());
        assert_eq!(visual.debug_bounds(row), Some(bounds));
        visual.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds(branch).is_none());
    }
    visual.update(|window, _| window.remove_window());
}
