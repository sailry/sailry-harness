use super::*;
use core::prelude::v1::test;
use gpui_kit::component::{Root, ThemeMode};
use sailry_node_runtime::Node;
use sailry_protocol::Command;
use std::time::{Duration, Instant};

fn wait(cx: &mut VisualTestContext, shell: &Entity<Shell>, predicate: impl Fn(&State) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        cx.run_until_parked();
        if shell.read_with(cx, |shell, _| predicate(shell.live.as_ref().unwrap())) {
            break;
        }
        assert!(Instant::now() < deadline, "live view update deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
}
fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
}

#[gpui::test]
fn project_empty_state(cx: &mut TestAppContext) {
    use crate::preview::Page;
    use gpui_kit::component::WindowExt as _;

    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    let local = runtime
        .block_on(Node::start(directory.path().join("local")))
        .unwrap();
    let remote = runtime
        .block_on(Node::start(directory.path().join("remote")))
        .unwrap();
    let invitation = remote.link().invite().unwrap();
    runtime
        .block_on(local.link().pair(invitation.ticket()))
        .unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(Services {
            runtime: runtime.clone(),
            link: local.link(),
            local: local.local(),
            relay_enabled: false,
        });
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        entity = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = entity.unwrap();
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
    wait(visual, &shell, |live| {
        live.view.connected && live.hosts.len() == 2
    });
    click(visual, "project-add");
    assert!(visual.debug_bounds("project-editor").is_some());
    assert!(
        visual
            .debug_bounds("plugin-control-worktrees-location")
            .is_some()
    );
    click(visual, "project-cancel");
    for (node, page, selector) in [
        (local.id(), Page::Files, "file-explorer"),
        (remote.id(), Page::Git, "git-changes"),
    ] {
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.select_live_host(node, window, cx);
            })
        });
        wait(visual, &shell, |live| {
            live.view.connected
                && live
                    .view
                    .snapshot
                    .as_ref()
                    .is_some_and(|view| view.node == node)
        });
        crate::conversation::live::tests::wait(visual, |cx| {
            shell
                .read(cx)
                .renderer_navigation(
                    if page == Page::Git {
                        sailry_protocol::plugin::desktop::ResourceKind::Git
                    } else {
                        sailry_protocol::plugin::desktop::ResourceKind::Documents
                    },
                    cx,
                )
                .is_some_and(|entry| entry.node == node)
        });
        visual.update(|window, cx| shell.update(cx, |shell, cx| shell.navigate(page, window, cx)));
        assert!(visual.debug_bounds(selector).is_none());
        assert!(visual.debug_bounds("toggle-details").is_none());
        let main = visual.debug_bounds("main-content").unwrap();
        assert_eq!(main.right(), px(1280.));
        let message = visual.debug_bounds("empty-resource_project_empty").unwrap();
        let button = visual.debug_bounds("empty-create-project").unwrap();
        assert!(button.top() > message.bottom());
        assert_eq!(button.center().x, main.center().x);
        click(visual, "empty-create-project");
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        click(visual, "project-cancel");
        assert_eq!(shell.read_with(visual, |shell, _| shell.page), Page::Plugin);
        assert!(visual.debug_bounds(selector).is_none());
        click(visual, "empty-create-project");
        click(visual, "project_name-input");
        visual.simulate_input("New workspace");
        click(visual, "project_path-input");
        visual.simulate_input(directory.path().to_str().unwrap());
        click(visual, "project-save");
        wait(visual, &shell, |live| {
            live.selected_project()
                .is_some_and(|project| project.name == "New workspace")
                && live.selected_worktree().is_some()
        });
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        crate::conversation::live::tests::wait(visual, |cx| {
            shell
                .read(cx)
                .extensions
                .as_ref()
                .is_some_and(|navigation| {
                    navigation.selected.as_ref().is_some_and(|entry| {
                        entry.node == node
                            && entry.package.name
                                == if page == Page::Files { "files" } else { "git" }
                    }) && navigation
                        .panel
                        .as_ref()
                        .is_some_and(|panel| panel.read(cx).resource_active())
                })
        });
        assert_eq!(shell.read_with(visual, |shell, _| shell.page), Page::Plugin);
        assert!(visual.debug_bounds("empty-create-project").is_none());
        assert!(visual.debug_bounds("plugin-panel").is_some());
        assert!(visual.debug_bounds("toggle-details").is_some());
    }
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.live = None));
    runtime.block_on(local.shutdown()).unwrap();
    runtime.block_on(remote.shutdown()).unwrap();
}

fn overview_layout(cx: &mut VisualTestContext, shell: &Entity<Shell>) {
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [760., 1280.] {
            let handle = cx.update(|window, cx| {
                crate::theme::select(Some(mode), window, cx);
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(width), px(820.)));
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let overview = cx.debug_bounds("live-overview").unwrap();
            for selector in [
                "host-name",
                "host-charts",
                "host-cpu",
                "host-gpu",
                "host-memory",
                "host-network",
                "host-disk",
            ] {
                let bounds = cx.debug_bounds(selector).unwrap();
                assert!(bounds.left() >= overview.left(), "{selector} left edge");
                assert!(bounds.right() <= overview.right(), "{selector} right edge");
                if !matches!(selector, "host-charts" | "host-network" | "host-disk") {
                    assert!(
                        bounds.bottom() <= overview.bottom(),
                        "{selector} bottom edge"
                    );
                }
            }
            let cpu = cx.debug_bounds("host-cpu").unwrap();
            let memory = cx.debug_bounds("host-memory").unwrap();
            let gpu = cx.debug_bounds("host-gpu").unwrap();
            let disk = cx.debug_bounds("host-disk").unwrap();
            let network = cx.debug_bounds("host-network").unwrap();
            assert_eq!(cpu.top(), gpu.top());
            assert_eq!(cpu.top(), memory.top());
            assert!(cpu.right() <= gpu.left());
            assert!(gpu.right() <= memory.left());
            assert_eq!(cpu.left(), disk.left());
            assert_eq!(memory.right(), disk.right());
            assert_eq!(network.left(), disk.left());
            assert_eq!(network.right(), disk.right());
            assert!(cpu.bottom() <= network.top());
            assert!(network.bottom() <= disk.top());
            for (chart, detail) in [
                ("host-cpu-chart", "host-cpu-detail"),
                ("host-gpu-chart", "host-gpu-detail"),
                ("host-memory-chart", "host-memory-detail"),
                ("host-disk-chart", "host-disk-detail"),
            ] {
                let gap = cx.debug_bounds(detail).unwrap().top()
                    - cx.debug_bounds(chart).unwrap().bottom();
                assert!((px(0.)..=px(8.)).contains(&gap), "chart details stay close");
            }
            assert!(cx.debug_bounds("host-process-search").is_some());
            assert!(cx.debug_bounds("host-activity-empty").is_none());
            assert!(cx.debug_bounds("host-processes").is_some());
            if width >= 1280.
                && let Some(last) = cx.debug_bounds("host-process-cell-4")
            {
                let table = cx.debug_bounds("host-processes").unwrap();
                assert!(
                    (table.right() - last.right()).abs() < px(24.),
                    "process columns fill the table"
                );
            }
            assert!(cx.debug_bounds("live-add-project").is_none());
            assert!(cx.debug_bounds("port-start").is_none());
            let remote = shell.read_with(cx, |shell, _| {
                let live = shell.live.as_ref().unwrap();
                live.selected != live.services.local.target()
            });
            assert_eq!(cx.debug_bounds("live-revoke").is_some(), remote);
        }
    }
}

fn welcome(cx: &mut VisualTestContext, shell: &Entity<Shell>) -> SharedString {
    assert_eq!(
        shell.read_with(cx, |shell, _| shell.page),
        crate::preview::Page::Conversation
    );
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [760., 1280.] {
            let handle = cx.update(|window, cx| {
                crate::theme::select(Some(mode), window, cx);
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(width), px(820.)));
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let page = cx.debug_bounds("live-conversation").unwrap();
            for selector in [
                "conversation-welcome",
                "conversation-composer",
                "live-chat-input",
                "welcome_explore",
                "welcome_build",
                "welcome_review",
                "welcome_plan",
            ] {
                let bounds = cx.debug_bounds(selector).unwrap();
                assert!(
                    bounds.left() >= page.left() && bounds.right() <= page.right(),
                    "{selector} horizontal bounds"
                );
                assert!(bounds.bottom() <= page.bottom(), "{selector} bottom edge");
            }
            let surface = cx.debug_bounds("composer-surface").unwrap();
            let multiple =
                shell.read_with(cx, |shell, _| shell.live.as_ref().unwrap().hosts.len() > 1);
            assert_eq!(cx.debug_bounds("composer-host").is_some(), multiple);
            assert!(
                cx.debug_bounds("plugin-control-worktrees-location")
                    .is_some()
            );
            let context = cx.debug_bounds("composer-context-bar").unwrap();
            assert!(context.left() >= surface.left() && context.right() <= surface.right());
            assert!(cx.debug_bounds("composer-project").is_none());
        }
    }
    for (selector, prompt) in [
        ("welcome_explore", "welcome_explore_prompt"),
        ("welcome_build", "welcome_build_prompt"),
        ("welcome_review", "welcome_review_prompt"),
        ("welcome_plan", "welcome_plan_prompt"),
    ] {
        click(cx, selector);
        assert_eq!(
            shell.read_with(cx, |shell, cx| shell
                .current_chat()
                .unwrap()
                .read(cx)
                .draft(cx)),
            tr(prompt)
        );
    }
    click(cx, "live-chat-mode");
    click(cx, "composer_mode_plan-option");
    click(cx, "live-chat-permission");
    click(cx, "composer_permission_full-option");
    let text = shell.read_with(cx, |shell, cx| {
        shell.current_chat().unwrap().read(cx).draft(cx)
    });
    assert!(!text.is_empty());
    assert!(shell.read_with(cx, |shell, _| shell.host_monitor.is_some()));
    text
}

#[gpui::test]
fn pairs_and_switches_registered_hosts(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    let local = runtime
        .block_on(Node::start(directory.path().join("local")))
        .unwrap();
    let remote = runtime
        .block_on(Node::start(directory.path().join("remote")))
        .unwrap();
    // GPUI's test selector API requires static strings; two bounded fixture IDs.
    let remote_selector =
        Box::leak(format!("live-host-{}", node_key(remote.id())).into_boxed_str());
    let local_selector = Box::leak(format!("live-host-{}", node_key(local.id())).into_boxed_str());
    for (node, name) in [(&local, "Local real"), (&remote, "Remote real")] {
        let client = Client::new(node.local());
        runtime
            .block_on(client.execute(client.prepare(Command::RegisterProject {
                name: name.into(),
                path: directory.path().to_str().unwrap().into(),
            })))
            .unwrap();
    }
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(Services {
            runtime: runtime.clone(),
            link: local.link(),
            local: local.local(),
            relay_enabled: false,
        });
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        entity = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = entity.unwrap();
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
    wait(visual, &shell, |live| {
        live.view.connected
            && live
                .info
                .as_ref()
                .is_some_and(|info| info.node == local.id())
            && live
                .view
                .snapshot
                .as_ref()
                .is_some_and(|view| view.projects.len() == 1)
    });
    assert_eq!(
        shell.read_with(visual, |shell, _| shell
            .live
            .as_ref()
            .unwrap()
            .view
            .snapshot
            .as_ref()
            .unwrap()
            .projects[0]
            .name
            .clone()),
        "Local real"
    );
    let text = welcome(visual, &shell);
    let draft = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
    click(visual, local_selector);
    overview_layout(visual, &shell);
    let invitation = remote.link().invite().unwrap();
    runtime
        .block_on(local.link().pair(invitation.ticket()))
        .unwrap();
    wait(visual, &shell, |live| live.hosts.len() == 2);
    click(visual, remote_selector);
    wait(visual, &shell, |live| {
        live.view.connected
            && live
                .info
                .as_ref()
                .is_some_and(|info| info.node == remote.id())
            && live
                .view
                .snapshot
                .as_ref()
                .is_some_and(|view| view.node == remote.id())
    });
    assert_eq!(
        shell.read_with(visual, |shell, _| shell
            .live
            .as_ref()
            .unwrap()
            .view
            .snapshot
            .as_ref()
            .unwrap()
            .projects[0]
            .name
            .clone()),
        "Remote real"
    );
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.new_live_conversation(window, cx));
    });
    click(visual, "composer-host");
    visual.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
    visual.simulate_keystrokes("down down enter");
    super::conversation::tests::wait(visual, |cx| {
        draft.read(cx).binding().client.target() == remote.id() && draft.read(cx).connected()
    });
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone()),
        draft
    );
    assert_eq!(draft.read_with(visual, |view, cx| view.draft(cx)), text);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let project = shell
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .unwrap()
                .projects[0]
                .id;
            shell.select_live_project(project, window, cx);
            shell.new_live_conversation(window, cx);
        });
    });
    super::conversation::tests::wait(visual, |cx| draft.read(cx).binding().project.is_some());
    assert_eq!(draft.read_with(visual, |view, cx| view.draft(cx)), text);
    assert!(draft.read_with(visual, |view, _| view.session().is_none()));
    click(visual, remote_selector);
    wait(visual, &shell, |live| live.info.is_some());
    visual.update(|_, cx| {
        shell.update(cx, |shell, _| {
            let info = shell.live.as_mut().unwrap().info.as_mut().unwrap();
            info.name = Some("Long execution host name ".repeat(20));
            info.memory = None;
            info.logical_cpus = None;
        });
    });
    overview_layout(visual, &shell);
    click(visual, "project-add");
    click(visual, "project_name-input");
    visual.simulate_input("Registered through UI");
    click(visual, "project_path-input");
    visual.simulate_input(remote.profile().to_str().unwrap());
    click(visual, "project-save");
    wait(visual, &shell, |live| {
        live.selected_project()
            .is_some_and(|project| project.name == "Registered through UI")
    });
    assert_eq!(
        shell.read_with(visual, |shell, _| shell
            .live
            .as_ref()
            .unwrap()
            .view
            .snapshot
            .as_ref()
            .unwrap()
            .worktrees
            .len()),
        2
    );
    click(visual, local_selector);
    wait(visual, &shell, |live| {
        live.view.connected
            && live
                .info
                .as_ref()
                .is_some_and(|info| info.node == local.id())
            && live
                .view
                .snapshot
                .as_ref()
                .is_some_and(|view| view.node == local.id())
    });
    assert_eq!(
        shell.read_with(visual, |shell, _| shell
            .live
            .as_ref()
            .unwrap()
            .view
            .snapshot
            .as_ref()
            .unwrap()
            .projects
            .len()),
        1
    );
    click(visual, remote_selector);
    wait(visual, &shell, |live| {
        live.view.connected && live.selected == remote.id()
    });
    click(visual, "live-revoke");
    crate::prompts::tests::answer(visual, "live_revoke");
    wait(visual, &shell, |live| {
        live.selected == local.id() && live.hosts.len() == 1 && live.view.connected
    });
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.live = None));
    runtime.block_on(local.shutdown()).unwrap();
    runtime.block_on(remote.shutdown()).unwrap();
}
