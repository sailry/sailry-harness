use super::*;
use crate::preview::Page;
use sailry_protocol::{Output, ProjectId, WorktreeId};

fn select(shell: &Entity<Shell>, visual: &mut VisualTestContext, project: ProjectId) {
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.live_resource_action(
                &crate::live::menus::Dispatch {
                    node: shell.live.as_ref().unwrap().selected,
                    target: crate::live::menus::Target::Project(project),
                    command: crate::live::menus::Command::Open,
                },
                window,
                cx,
            );
        })
    });
    wait(visual, |cx| {
        shell.read(cx).live.as_ref().unwrap().project == Some(project)
    });
}

fn open(shell: &Entity<Shell>, visual: &mut VisualTestContext) -> Entity<Panel> {
    let entry = shell.read_with(visual, |shell, cx| {
        shell
            .extension_entries(cx)
            .into_iter()
            .find(|entry| entry.package.name == "project-summary")
            .unwrap()
    });
    click(visual, Box::leak(entry.selector().into_boxed_str()));
    shell.read_with(visual, |shell, _| {
        shell.extensions.as_ref().unwrap().panel.clone().unwrap()
    })
}

fn loaded(panel: &Entity<Panel>, visual: &mut VisualTestContext, tree: WorktreeId, text: &str) {
    wait(visual, |cx| {
        let description = snapshot(panel, cx);
        description.contains(&format!("scope-{tree}")) && description.contains(text)
    });
    assert_eq!(
        panel.read_with(visual, |panel, _| panel.binding.worktree),
        Some(tree)
    );
}

fn geometry(shell: &Entity<Shell>, visual: &mut VisualTestContext, visible: bool, minimum: f32) {
    let actual = visual.update(|window, cx| {
        let shell = shell.read(cx);
        let navigation = shell
            .plugin_workspace(cx)
            .is_some_and(|workspace| workspace.read(cx).has_navigation());
        let geometry = shell.frame_geometry(navigation, shell.has_resource_panel(cx), window, cx);
        (geometry.visible, geometry.window_minimum())
    });
    // The native header's selected state and the rendered pane use this same geometry.
    assert_eq!(actual, (visible, size(px(minimum), px(560.))));
    assert_eq!(visual.debug_bounds("workspace-add-ten").is_some(), visible);
}

fn settle(visual: &mut VisualTestContext) {
    let duration = visual.update(|_, cx| cx.theme().motion_tokens().duration_normal);
    visual
        .executor()
        .advance_clock(duration + std::time::Duration::from_millis(10));
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
}

fn resize_details(visual: &mut VisualTestContext, delta: f32) {
    let details = visual.debug_bounds("workspace-details").unwrap();
    let divider = point(details.left(), details.center().y);
    visual.simulate_mouse_move(divider, None, Modifiers::default());
    visual.simulate_mouse_down(divider, MouseButton::Left, Modifiers::default());
    for step in 1..=4 {
        visual.simulate_mouse_move(
            divider + point(px(delta * step as f32 / 4.), px(0.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        wait(visual, |_| true);
    }
    visual.simulate_mouse_up(
        divider + point(px(delta), px(0.)),
        MouseButton::Left,
        Modifiers::default(),
    );
}

#[gpui::test]
fn shares_controller_and_preserves_captured_worktrees(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let root = fixture.directory.path().join("project/package");
        let path = root.join("plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let desktop = &mut manifest["extensions"]["dev.sailry.platform"]["desktop"];
        desktop["navigation"] =
            serde_json::json!({"label":"Workspace","icon":"reicon:folders/folder"});
        desktop["navigation_options"] =
            serde_json::json!({"pinned":true,"target":"worktree","details":true});
        std::fs::write(path, manifest.to_string()).unwrap();
        std::fs::write(
            root.join("dev.sailry.platform/desktop/main.js"),
            include_str!("workspace.js"),
        )
        .unwrap();
        let package = fixture.install(0);
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let second = fixture.directory.path().join("second");
        std::fs::create_dir(&second).unwrap();
        git2::Repository::init(&second).unwrap();
        std::fs::write(second.join("notes.txt"), "Second workspace").unwrap();
        let Output::Project(second) = fixture.execute(Command::RegisterProject {
            name: "Second".into(),
            path: second.to_str().unwrap().into(),
        }) else {
            panic!("project expected");
        };
        cx.update(|cx| cx.set_global(fixture.services(remote)));
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| Shell::new(window, cx));
            owner = Some(shell.clone());
            Root::new(shell, window, cx)
        });
        let shell = owner.unwrap();
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1440.), px(900.)));
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .hosts
                .contains_key(&fixture.node.id())
        });
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(fixture.node.id(), cx)
            })
        });
        wait(visual, |cx| {
            shell
                .read(cx)
                .extension_entries(cx)
                .iter()
                .any(|entry| entry.package == package.summary.reference())
        });
        let empty = open(&shell, visual);
        assert!(shell.read_with(visual, |shell, cx| shell.needs_project()
            && !shell.has_resource_panel(cx)));
        assert!(empty.read_with(visual, |panel, _| panel.mounted.is_none()));
        assert!(visual.debug_bounds("shell-navigation").is_none());
        assert!(visual.debug_bounds("header-sidebar-toggle").is_none());
        assert!(visual.debug_bounds("activity-host-filter").is_none());
        drop(empty);

        select(&shell, visual, fixture.binding.project.unwrap());
        let first = open(&shell, visual);
        loaded(
            &first,
            visual,
            fixture.binding.worktree.unwrap(),
            "Complete",
        );
        assert_eq!(
            first.read_with(visual, |panel, _| panel.binding.project),
            fixture.binding.project
        );
        for selector in [
            "shell-module-header",
            "resource-title",
            "header-actions",
            "toggle-details",
        ] {
            assert!(
                visual.debug_bounds(selector).is_some(),
                "missing {selector}"
            );
        }
        assert_eq!(
            shell.read_with(visual, |shell, cx| shell
                .plugin_workspace(cx)
                .unwrap()
                .read(cx)
                .details_label()),
            Some("Workspace details".into()),
        );
        assert_eq!(
            shell.read_with(visual, |shell, cx| shell
                .plugin_workspace(cx)
                .unwrap()
                .read(cx)
                .width),
            px(320.)
        );
        let constraints = shell.read_with(visual, |shell, cx| {
            shell.plugin_workspace(cx).unwrap().read(cx).ranges()
        });
        assert_eq!(constraints.navigation, px(280.)..px(320.));
        assert_eq!(constraints.details, px(300.)..px(480.));
        geometry(&shell, visual, true, 836.);
        assert!(visual.debug_bounds("shell-navigation").is_none());
        assert!(visual.debug_bounds("header-sidebar-toggle").is_none());
        assert_eq!(
            visual.debug_bounds("main-content").unwrap().left(),
            px(crate::preview::RAIL_WIDTH),
        );
        visual.simulate_keystrokes("secondary-b");
        assert!(shell.read_with(visual, |shell, _| shell.layout.sidebar_open));
        let sidebar_width = shell.read_with(visual, |shell, _| shell.layout.sidebar_width);
        click(visual, "workspace-navigation");
        wait(visual, |cx| {
            snapshot(&first, cx).contains("navigation-none")
        });
        assert!(visual.debug_bounds("shell-navigation").is_none());
        assert!(visual.debug_bounds("header-sidebar-toggle").is_none());
        assert_eq!(
            visual.debug_bounds("main-content").unwrap().left(),
            px(crate::preview::RAIL_WIDTH),
        );
        visual.simulate_keystrokes("secondary-b");
        assert!(shell.read_with(visual, |shell, _| shell.layout.sidebar_open));
        click(visual, "workspace-navigation");
        wait(visual, |cx| {
            snapshot(&first, cx).contains("navigation-resource")
        });
        settle(visual);
        let navigation = visual.debug_bounds("workspace-navigation-slot").unwrap();
        assert!((navigation.size.width - px(280.)).abs() <= px(1.));
        assert_eq!(navigation.top(), px(crate::preview::HEADER_HEIGHT));
        assert_eq!(
            navigation.right(),
            visual.debug_bounds("main-content").unwrap().left(),
        );
        geometry(&shell, visual, true, 1116.);
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.layout.sidebar_width),
            sidebar_width,
        );
        click(visual, "workspace-sidebar-action");
        wait(visual, |cx| {
            snapshot(&first, cx).contains("sidebar-count-1")
        });
        click(visual, "header-sidebar-toggle");
        wait(visual, |cx| !shell.read(cx).layout.sidebar_open);
        click(visual, "workspace-sidebar-availability");
        wait(visual, |cx| snapshot(&first, cx).contains("sidebar-false"));
        assert!(visual.debug_bounds("header-sidebar-toggle").is_none());
        click(visual, "workspace-sidebar-availability");
        wait(visual, |cx| snapshot(&first, cx).contains("sidebar-true"));
        assert!(visual.debug_bounds("header-sidebar-toggle").is_some());
        assert!(visual.debug_bounds("workspace-navigation-slot").is_none());
        assert_eq!(
            shell.read_with(visual, |shell, _| (
                shell.layout.sidebar_open,
                shell.layout.sidebar_width,
            )),
            (false, sidebar_width),
        );
        geometry(&shell, visual, true, 836.);
        click(visual, "header-sidebar-toggle");
        wait(visual, |cx| shell.read(cx).layout.sidebar_open);
        click(visual, "workspace-navigation");
        wait(visual, |cx| {
            snapshot(&first, cx).contains("navigation-default")
        });
        assert!(visual.debug_bounds("workspace-navigation-slot").is_none());
        // Measure the divider after the native sidebar transition has settled.
        settle(visual);
        let main = visual.debug_bounds("workspace-main").unwrap();
        let details = visual.debug_bounds("workspace-details").unwrap();
        assert!(
            (main.left() - px(crate::preview::RAIL_WIDTH)).abs() <= px(1.),
            "hidden navigation still occupies space: {main:?}",
        );
        assert!(main.right() <= details.left());
        assert_eq!(
            main.top() - details.top(),
            px(crate::preview::HEADER_HEIGHT)
        );
        resize_details(visual, -40.);
        wait(visual, |cx| {
            shell.read(cx).plugin_workspace(cx).unwrap().read(cx).width > px(320.)
        });
        resize_details(visual, 100.);
        wait(visual, |cx| {
            shell.read(cx).plugin_workspace(cx).unwrap().read(cx).width == px(300.)
        });
        assert!(
            (visual.debug_bounds("workspace-details").unwrap().size.width - px(300.)).abs()
                <= px(1.),
        );
        resize_details(visual, -60.);
        wait(visual, |cx| {
            shell.read(cx).plugin_workspace(cx).unwrap().read(cx).width == px(360.)
        });
        let width = shell.read_with(visual, |shell, cx| {
            shell.plugin_workspace(cx).unwrap().read(cx).width
        });
        assert!((width - px(360.)).abs() <= px(1.));
        click(visual, "workspace-increment");
        wait(visual, |cx| {
            snapshot(&first, cx).contains("details-count-1")
        });
        click(visual, "workspace-add-ten");
        wait(visual, |cx| snapshot(&first, cx).contains("main-count-11"));
        let root = first.read_with(visual, |panel, _| {
            panel.mounted.as_ref().unwrap().root.entity_id()
        });
        click(visual, "toggle-details");
        wait(visual, |cx| {
            !shell.read(cx).plugin_workspace(cx).unwrap().read(cx).open
        });
        assert!(visual.debug_bounds("workspace-add-ten").is_none());
        geometry(&shell, visual, false, 760.);
        click(visual, "workspace-availability");
        wait(visual, |cx| {
            snapshot(&first, cx).contains("availability-false")
                && !shell.read(cx).has_resource_panel(cx)
        });
        assert!(visual.debug_bounds("toggle-details").is_none());
        assert!(visual.debug_bounds("workspace-add-ten").is_none());
        assert_eq!(
            shell.read_with(visual, |shell, cx| {
                let workspace = shell.plugin_workspace(cx).unwrap();
                let state = workspace.read(cx);
                (state.open, state.width)
            }),
            (false, width),
        );
        click(visual, "workspace-availability");
        wait(visual, |cx| {
            snapshot(&first, cx).contains("availability-true")
                && shell.read(cx).has_resource_panel(cx)
        });
        assert!(visual.debug_bounds("toggle-details").is_some());
        assert!(visual.debug_bounds("workspace-add-ten").is_none());
        click(visual, "workspace-increment");
        wait(visual, |cx| snapshot(&first, cx).contains("main-count-12"));
        click(visual, "toggle-details");
        wait(visual, |cx| {
            shell.read(cx).plugin_workspace(cx).unwrap().read(cx).open
        });
        click(visual, "workspace-availability");
        wait(visual, |cx| !shell.read(cx).has_resource_panel(cx));
        assert!(visual.debug_bounds("workspace-add-ten").is_none());
        geometry(&shell, visual, false, 760.);
        click(visual, "workspace-availability");
        wait(visual, |cx| shell.read(cx).has_resource_panel(cx));
        assert!(visual.debug_bounds("workspace-add-ten").is_some());
        geometry(&shell, visual, true, 836.);
        assert_eq!(
            first.read_with(visual, |panel, _| panel
                .mounted
                .as_ref()
                .unwrap()
                .root
                .entity_id()),
            root,
        );
        assert_eq!(
            shell.read_with(visual, |shell, cx| {
                let workspace = shell.plugin_workspace(cx).unwrap();
                let state = workspace.read(cx);
                (state.open, state.width)
            }),
            (true, width),
        );
        let left_width = visual.debug_bounds("main-content").unwrap().left();
        let minimum = shell.read_with(visual, |shell, cx| {
            px(shell.layout.main_min(Page::Plugin)).max(first.read(cx).min_content_width(cx))
        });
        // Default navigation leaves only the rail; constrain the actual fit threshold.
        let narrow = left_width + minimum + width - px(40.);
        for (viewport, visible) in [
            (size(narrow, px(800.)), false),
            (size(px(1440.), px(900.)), true),
        ] {
            visual.simulate_window_resize(handle, viewport);
            wait(visual, |_| true);
            assert_eq!(visual.debug_bounds("workspace-add-ten").is_some(), visible,);
            assert_eq!(
                shell.read_with(visual, |shell, cx| {
                    let workspace = shell.plugin_workspace(cx).unwrap();
                    let state = workspace.read(cx);
                    (state.open, state.width)
                }),
                (true, width),
            );
        }
        click(visual, "workspace-minimum");
        wait(visual, |cx| {
            first.read(cx).min_content_width(cx) == px(1000.)
        });
        for (viewport, visible, minimum) in [
            (size(px(1400.), px(900.)), false, 1056.),
            (size(px(1440.), px(900.)), true, 1356.),
        ] {
            visual.simulate_window_resize(handle, viewport);
            wait(visual, |_| true);
            geometry(&shell, visual, visible, minimum);
            assert_eq!(
                shell.read_with(visual, |shell, cx| {
                    let workspace = shell.plugin_workspace(cx).unwrap();
                    let state = workspace.read(cx);
                    (state.open, state.width)
                }),
                (true, width),
            );
        }
        click(visual, "workspace-minimum");
        wait(visual, |cx| {
            first.read(cx).min_content_width(cx) == px(480.)
        });
        geometry(&shell, visual, true, 836.);
        click(visual, "workspace-add-ten");
        wait(visual, |cx| snapshot(&first, cx).contains("main-count-22"));

        select(&shell, visual, second.id);
        let second_tree = shell.read_with(visual, |shell, _| {
            shell.live.as_ref().unwrap().selected_worktree().unwrap().id
        });
        let other = open(&shell, visual);
        loaded(&other, visual, second_tree, "Second workspace");
        assert_ne!(first.entity_id(), other.entity_id());
        assert_eq!(
            first.read_with(visual, |panel, _| panel.binding.worktree),
            fixture.binding.worktree
        );
        assert!(
            visual
                .update(|_, cx| snapshot(&other, cx))
                .contains("main-count-0")
        );
        select(&shell, visual, fixture.binding.project.unwrap());
        let restored = open(&shell, visual);
        assert_eq!(restored.entity_id(), first.entity_id());
        assert_eq!(
            restored.read_with(visual, |panel, _| panel
                .mounted
                .as_ref()
                .unwrap()
                .root
                .entity_id()),
            root
        );
        assert_eq!(
            restored.read_with(visual, |panel, cx| panel
                .workspace
                .as_ref()
                .unwrap()
                .read(cx)
                .width),
            width,
        );
        assert!(
            visual
                .update(|_, cx| snapshot(&restored, cx))
                .contains("main-count-22")
        );
        drop(restored);
        let hidden = other.downgrade();
        drop(other);
        let Output::Plugin(disabled) = fixture.execute(Command::SetPluginEnabled {
            name: package.summary.name.clone(),
            expected_revision: package.summary.revision,
            enabled: false,
        }) else {
            panic!("plugin expected");
        };
        wait(visual, |cx| {
            first.read(cx).mounted.is_none() && hidden.upgrade().is_none()
        });
        assert!(visual.debug_bounds("workspace-increment").is_none());
        assert!(visual.debug_bounds("workspace-add-ten").is_none());
        fixture.execute(Command::SetPluginEnabled {
            name: disabled.summary.name,
            expected_revision: disabled.summary.revision,
            enabled: true,
        });
        wait(visual, |cx| {
            let panel = first.read(cx);
            panel.error.is_none() && !panel.loading && panel.mounted.is_some()
        });
        loaded(
            &first,
            visual,
            fixture.binding.worktree.unwrap(),
            "Complete",
        );
        assert_ne!(
            first.read_with(visual, |panel, _| panel
                .mounted
                .as_ref()
                .unwrap()
                .root
                .entity_id()),
            root
        );
        let released = first.downgrade();
        drop(first);
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .live
                    .as_mut()
                    .unwrap()
                    .select(fixture.controller.id(), cx)
            })
        });
        wait(visual, |cx| {
            shell.read(cx).page == Page::Host && released.upgrade().is_none()
        });
        visual.update(|window, _| window.remove_window());
        drop(shell);
        fixture.close();
    }
}
