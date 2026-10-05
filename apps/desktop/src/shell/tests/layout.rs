use super::*;
fn click(cx: &mut VisualTestContext, selector: &'static str) {
    super::workspace::click(cx, selector);
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn navigation_fits_without_changing_its_saved_width(cx: &mut TestAppContext) {
    let (shell, mut visual) = setup(cx);
    let handle = visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.layout.sidebar_width = 320.;
            shell.navigate(Page::Settings, window, cx);
        });
        window.window_handle()
    });
    for (width, navigation_width) in [(760., 224.), (1280., 320.)] {
        visual.simulate_window_resize(handle, size(px(width), px(820.)));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let navigation = visual.debug_bounds("shell-navigation").unwrap();
        let main = visual.debug_bounds("main-content").unwrap();
        assert!((navigation.size.width - px(navigation_width)).abs() < px(1.));
        assert!(main.size.width >= px(MAIN_MIN));
        assert_eq!(
            shell.read_with(&visual, |shell, _| shell.layout.sidebar_width),
            320.
        );
    }
}

#[gpui::test]
fn optional_pane_releases_minimum(cx: &mut TestAppContext) {
    let (shell, mut visual) = setup(cx);
    let handle = visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.layout.sidebar_width = crate::sidebar::WIDTH_RANGE.start;
            shell.layout.panel_open[0] = true;
            shell.layout.panel_width[0] = crate::resources::MIN_WIDTH;
            shell.layout.conversation_panel_resized = true;
            shell.side_resource = Some(crate::resources::SideResource::Link(
                "https://example.test".into(),
            ));
            cx.notify();
        });
        window.window_handle()
    });
    let boundary = px(RAIL_WIDTH
        + crate::sidebar::WIDTH_RANGE.start
        + crate::conversation::MIN_WIDTH
        + crate::resources::MIN_WIDTH);
    for (width, visible, minimum) in [
        (px(1280.), true, boundary),
        (boundary, false, px(760.)),
        (px(1280.), true, boundary),
    ] {
        visual.simulate_window_resize(handle, size(width, px(820.)));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        visual.update(|window, cx| {
            let shell = shell.read(cx);
            let geometry = shell.frame_geometry(true, shell.has_resource_panel(cx), window, cx);
            assert_eq!(geometry.visible, visible);
            assert_eq!(geometry.window_minimum().width, minimum);
            assert!(shell.layout.panel_open[0]);
            assert_eq!(shell.layout.panel_width[0], crate::resources::MIN_WIDTH);
        });
        assert_eq!(
            visual.debug_bounds("resource-link-preview").is_some(),
            visible
        );
    }
}

#[gpui::test]
fn feature_navigation_and_headers(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let rail = cx.debug_bounds("shell-feature-rail").unwrap();
    let navigation = cx.debug_bounds("shell-navigation").unwrap();
    let header = cx.debug_bounds("shell-module-header").unwrap();
    assert_eq!(rail.size.width, px(RAIL_WIDTH));
    assert_eq!(rail.top(), px(HEADER_HEIGHT));
    assert_eq!(rail.right(), navigation.left());
    assert!((navigation.right() - header.left()).abs() <= px(1.));
    assert_eq!(header.top(), px(0.));
    let corner = cx.debug_bounds("shell-corner-material").unwrap();
    assert_eq!(corner.left(), rail.right());
    assert_eq!(corner.top(), header.bottom());
    assert_eq!(corner.size.width, cx.update(|_, cx| cx.theme().radius_lg));
    assert!(cx.debug_bounds("shell-activity-slot").is_none());
    assert!(cx.debug_bounds("activity-open").is_none());
    assert!(
        cx.debug_bounds("notifications-open").unwrap().bottom()
            < cx.debug_bounds("sidebar-search").unwrap().top()
    );
    let details = cx.debug_bounds("toggle-details").unwrap();
    assert!(header.contains(&details.origin));
    assert!(details.bottom() <= header.bottom());
    assert!(details.right() <= header.right());
    assert!(cx.debug_bounds("sidebar-brand").is_none());
    let session = cx.update(|_, cx| shell.read(cx).session);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.navigate(Page::Activity, window, cx));
    });
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(cx.update(|_, cx| shell.read(cx).page), Page::Activity);
    assert!(cx.debug_bounds("shell-navigation").is_none());
    assert!(cx.debug_bounds("header-sidebar-toggle").is_none());
    assert!(cx.update(|_, cx| shell.read(cx).layout.sidebar_open));
    click(&mut cx, "navigation-conversation");
    assert!(cx.debug_bounds("shell-navigation").is_some());
    click(&mut cx, "navigation-files");
    let tree = cx.debug_bounds("file-explorer").unwrap();
    let main = cx.debug_bounds("main-content").unwrap();
    assert!(main.right() <= tree.left());
    assert_eq!(main.left(), px(RAIL_WIDTH));
    assert_eq!(cx.debug_bounds("shell-corner-material").unwrap(), corner);
    assert!(cx.debug_bounds("shell-navigation").is_none());
    assert!(cx.debug_bounds("header-sidebar-toggle").is_none());
    cx.simulate_keystrokes("secondary-b");
    assert!(cx.update(|_, cx| shell.read(cx).layout.sidebar_open));
    assert!(cx.debug_bounds("session-0").is_none());
    click(&mut cx, "navigation-conversation");
    assert_eq!(cx.update(|_, cx| shell.read(cx).session), session);
    assert!(cx.debug_bounds("session-0").is_some());
    click(&mut cx, "header-sidebar-toggle");
    assert!(cx.debug_bounds("shell-navigation").is_none());
    let rail = cx.debug_bounds("shell-feature-rail").unwrap();
    assert_eq!(rail.size.width, px(RAIL_WIDTH));
    assert_eq!(cx.debug_bounds("shell-corner-material").unwrap(), corner);
    let toggle = cx.debug_bounds("header-sidebar-toggle").unwrap();
    if cfg!(target_os = "macos") {
        assert!(toggle.left() >= px(80.));
    }
    click(&mut cx, "navigation-git");
    assert_eq!(cx.update(|_, cx| shell.read(cx).page), Page::Git);
    assert!(cx.debug_bounds("header-sidebar-toggle").is_none());
    let changes = cx.debug_bounds("git-changes").unwrap();
    assert!(cx.debug_bounds("main-content").unwrap().right() <= changes.left());
    click(&mut cx, "navigation-conversation");
    assert!(cx.debug_bounds("shell-navigation").is_none());
    click(&mut cx, "header-sidebar-toggle");
    assert!(cx.debug_bounds("shell-navigation").is_some());
    let outline = cx.debug_bounds("shell-content-outline").unwrap();
    assert_eq!(outline.left(), px(RAIL_WIDTH));
    assert_eq!(outline.top(), px(HEADER_HEIGHT));
    assert_eq!(
        outline.right(),
        cx.debug_bounds("shell-material").unwrap().right()
    );
}

#[gpui::test]
fn short_project_content_does_not_reserve_height(cx: &mut TestAppContext) {
    let (shell, mut visual) = setup(cx);
    visual.update(|_, cx| {
        crate::preferences::update(cx, |data| data.sidebar_metrics = true);
        shell.update(cx, |shell, cx| {
            shell
                .workspace
                .sessions
                .retain(|key, _| key.0 != 0 || key.1 == 0);
            shell.workspace.terminals.clear();
            cx.notify();
        });
    });
    let handle = visual.update(|window, _| window.window_handle());
    let mut natural_height = None;
    for height in [560., 820.] {
        visual.simulate_window_resize(handle, size(px(1280.), px(height)));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let projects = visual.debug_bounds("projects-heading").unwrap();
        let viewport = visual.debug_bounds("sidebar-projects-viewport").unwrap();
        let first = visual.debug_bounds("sidebar-project").unwrap();
        let last = visual.debug_bounds("session-0").unwrap();
        let recent = visual.debug_bounds("recent-toggle").unwrap();
        assert!(projects.bottom() <= viewport.top());
        assert!(viewport.top() <= first.top());
        assert!(last.bottom() <= viewport.bottom());
        assert!(viewport.bottom() - last.bottom() <= px(4.));
        assert!(viewport.bottom() <= recent.top());
        assert!(recent.top() - viewport.bottom() <= px(8.));
        assert!(recent.bottom() <= visual.debug_bounds("sidebar-footer").unwrap().top());
        if let Some(height) = natural_height {
            assert!((viewport.size.height - height).abs() <= px(1.));
        }
        natural_height = Some(viewport.size.height);
    }
}

#[gpui::test]
fn project_scrolling_preserves_hosts(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let handle = cx.update(|window, cx| {
        crate::preferences::update(cx, |data| data.sidebar_metrics = true);
        shell.update(cx, |shell, cx| {
            let owner = shell.workspace.owner(0);
            for _ in 0..30 {
                shell.workspace.create_session(owner);
            }
            cx.notify();
        });
        window.window_handle()
    });
    for height in [560., 820.] {
        cx.simulate_window_resize(handle, size(px(1000.), px(height)));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let viewport = cx.debug_bounds("sidebar-projects-viewport").unwrap();
        let fixed = [
            "new-conversation",
            "hosts-label",
            "host-0",
            "host-1",
            "projects-label",
            "sidebar-footer",
        ]
        .map(|selector| (selector, cx.debug_bounds(selector).unwrap()));
        assert!(viewport.top() >= fixed[4].1.bottom());
        assert!(viewport.bottom() <= fixed[5].1.top());
        let activity_height = cx.debug_bounds("sidebar-active").unwrap().bottom()
            - cx.debug_bounds("active-toggle").unwrap().top();
        assert!(
            viewport.size.height + activity_height >= px(199.5),
            "viewport {viewport:?}, height {height}"
        );
        for delta in [5000., -5000.] {
            let before = cx.debug_bounds("session-0").unwrap();
            cx.simulate_event(ScrollWheelEvent {
                position: viewport.center(),
                delta: ScrollDelta::Pixels(point(px(0.), px(delta))),
                ..Default::default()
            });
            cx.run_until_parked();
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let after = cx.debug_bounds("session-0").unwrap();
            if delta < 0. {
                assert!(after.top() < before.top());
                assert!(after.top() >= viewport.top());
                assert!(after.bottom() <= viewport.bottom());
            }
            for (selector, bounds) in fixed {
                assert_eq!(cx.debug_bounds(selector).unwrap(), bounds);
            }
        }
        let session = cx.debug_bounds("session-0").unwrap();
        cx.simulate_event(ScrollWheelEvent {
            position: fixed[2].1.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(5000.))),
            ..Default::default()
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(cx.debug_bounds("session-0").unwrap(), session);
        click(&mut cx, "session-0");
        assert_eq!(cx.update(|_, cx| shell.read(cx).session), 0);
    }
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.sidebar.recent_preview = shell
                .workspace
                .sessions
                .keys()
                .copied()
                .filter(|key| key.0 == shell.host)
                .rev()
                .collect();
            cx.notify();
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("sidebar-recent-viewport").is_none());
    click(&mut cx, "recent-toggle");
    let recent = cx.debug_bounds("sidebar-recent-viewport").unwrap();
    let project = cx.debug_bounds("sidebar-projects-viewport").unwrap();
    let host = cx.debug_bounds("host-0").unwrap();
    let project_row = cx.debug_bounds("session-0").unwrap();
    assert!(project.bottom() <= recent.top());
    assert!(recent.bottom() <= cx.debug_bounds("sidebar-footer").unwrap().top());
    let before = cx.debug_bounds("recent-preview-0").unwrap();
    cx.simulate_event(ScrollWheelEvent {
        position: recent.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-5000.))),
        ..Default::default()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let after = cx.debug_bounds("recent-preview-0").unwrap();
    assert!(after.top() < before.top());
    assert!(after.top() >= recent.top() && after.bottom() <= recent.bottom());
    assert_eq!(cx.debug_bounds("host-0").unwrap(), host);
    assert_eq!(cx.debug_bounds("session-0").unwrap(), project_row);
    click(&mut cx, "recent-toggle");
    assert!(cx.debug_bounds("recent-preview-0").is_none());
    assert!(
        cx.debug_bounds("sidebar-projects-viewport")
            .unwrap()
            .size
            .height
            > project.size.height
    );
    let count = cx.update(|_, cx| shell.read(cx).workspace.sessions.len());
    click(&mut cx, "new-conversation");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).workspace.sessions.len()),
        count + 1
    );
}

#[gpui::test]
fn container_tints_are_independent_of_window_opacity(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        window
            .root::<Root>()
            .unwrap()
            .unwrap()
            .update(cx, |root, cx| {
                root.style().background = Some(transparent_black().into());
                cx.notify();
            });
    });
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for (container, outer) in [(45., 100.), (73., 45.), (100., 73.)] {
            cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                crate::preferences::update(cx, |data| {
                    data.surfaces = Some(crate::preferences::Surfaces {
                        main: container,
                        sidebar: Some(outer),
                    })
                });
                shell.update(cx, |shell, cx| {
                    shell.layout.sidebar_open = true;
                    cx.notify();
                });
                cx.refresh_windows();
            });
            cx.run_until_parked();
            cx.update(|window, cx| window.draw(cx).clear(cx));
            for selector in ["shell-body", "shell-navigation"] {
                let bounds = cx.debug_bounds(selector).unwrap();
                cx.update(|window, cx| {
                    let bounds = bounds.scale(window.scale_factor());
                    let fills: Vec<_> = window
                        .painted_quads()
                        .into_iter()
                        .filter(|quad| {
                            !quad.background.is_transparent()
                                && quad.bounds.contains(&bounds.center())
                                && quad.content_mask.bounds.contains(&bounds.center())
                                && quad.bounds.size.width >= bounds.size.width
                                && quad.bounds.size.height >= bounds.size.height
                        })
                        .collect();
                    assert_eq!(
                        fills.len(),
                        1,
                        "{selector} must have exactly one background tint"
                    );
                    let expected = if selector == "shell-body" {
                        crate::theme::panel_background(cx)
                    } else {
                        crate::theme::navigation_background(cx)
                    };
                    assert_eq!(expected.a, container / 100.);
                    assert_eq!(fills[0].background, expected.into());
                });
            }
        }
    }
}
