use super::*;
use crate::activity::fixture::Fixture;
use std::time::Instant;

fn settle(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn group_icons_and_window_background(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    cx.executor().allow_parking();
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/great-sage");
    let loaded = crate::theme::package::Loaded::read(&source, true).unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(fixture.services());
        cx.global_mut::<crate::theme::Catalog>()
            .packages
            .push(loaded.into());
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        owner = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = owner.unwrap();
    let window = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(window, size(px(1280.), px(900.)));
    let deadline = Instant::now() + Duration::from_secs(15);
    while !shell.read_with(visual, |shell, _| {
        shell
            .live
            .as_ref()
            .is_some_and(|live| live.hosts.len() == 2)
    }) {
        settle(visual);
        assert!(Instant::now() < deadline, "sidebar hosts deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
    for index in 0..2 {
        let node = fixture.nodes[index].id();
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(node, cx);
                cx.notify();
            });
        });
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            settle(visual);
            if shell.read_with(visual, |shell, _| {
                shell.live.as_ref().is_some_and(|live| {
                    live.view
                        .snapshot
                        .as_ref()
                        .is_some_and(|snapshot| snapshot.node == node)
                })
            }) {
                break;
            }
            assert!(Instant::now() < deadline, "sidebar snapshot deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            visual.update(|window, cx| {
                crate::theme::apply(0, Some(window), cx);
                Theme::change(mode, Some(window), cx);
                shell.update(cx, |_, cx| cx.notify());
            });
            settle(visual);
            for key in ["hosts", "activity", "unassigned", "projects", "recent"] {
                let selector = Box::leak(format!("sidebar-group-{key}").into_boxed_str());
                let bounds = visual.debug_bounds(selector).unwrap();
                assert_eq!(bounds.size, size(px(16.), px(16.)));
                let before = shell.read_with(visual, |shell, _| match key {
                    "hosts" => shell.sidebar.hosts_closed,
                    "activity" => shell.sidebar.active_closed,
                    "unassigned" => shell.sidebar.unassigned_closed.contains(&node),
                    "projects" => shell.sidebar.projects_closed,
                    _ => shell.sidebar.recent_open,
                });
                for expected in [!before, before] {
                    let bounds = visual.debug_bounds(selector).unwrap();
                    visual.simulate_click(bounds.center(), Modifiers::default());
                    settle(visual);
                    let actual = shell.read_with(visual, |shell, _| match key {
                        "hosts" => shell.sidebar.hosts_closed,
                        "activity" => shell.sidebar.active_closed,
                        "unassigned" => shell.sidebar.unassigned_closed.contains(&node),
                        "projects" => shell.sidebar.projects_closed,
                        _ => shell.sidebar.recent_open,
                    });
                    assert_eq!(actual, expected, "group icon blocked disclosure");
                }
            }
            visual.update(|window, cx| {
                crate::theme::apply(1, Some(window), cx);
                shell.update(cx, |_, cx| cx.notify());
            });
            settle(visual);
            let background = visual.debug_bounds("theme-background.window").unwrap();
            assert!(visual.debug_bounds("theme-background.content").is_none());
            for selector in ["shell-navigation", "shell-body"] {
                let pane = visual.debug_bounds(selector).unwrap();
                assert!(background.contains(&pane.origin));
                assert!(background.right() >= pane.right());
                assert!(background.bottom() >= pane.bottom());
            }
            for key in ["hosts", "activity", "unassigned", "projects", "recent"] {
                let selector = Box::leak(format!("theme-icon.group.{key}").into_boxed_str());
                assert_eq!(
                    visual.debug_bounds(selector).unwrap().size,
                    size(px(16.), px(16.))
                );
            }
        }
    }
    visual.update(|window, _| window.remove_window());
}
