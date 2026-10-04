use super::*;
use crate::preview::Page;
use core::prelude::v1::test;
use sailry_node_runtime::Node;
use sailry_protocol::Command as Request;
use std::time::{Duration, Instant};
mod attention;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod projects;
mod sessions;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod unassigned;
mod worktrees;

fn wait(cx: &mut VisualTestContext, shell: &Entity<Shell>, node: NodeId) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if shell.read_with(cx, |shell, _| {
            let live = shell.live.as_ref().unwrap();
            live.hosts.len() == 2
                && live.view.connected
                && live
                    .view
                    .snapshot
                    .as_ref()
                    .is_some_and(|view| view.node == node)
        }) {
            break;
        }
        assert!(Instant::now() < deadline, "menu inventory deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[test]
fn limits_host_actions() {
    for local in [true, false] {
        assert_eq!(
            items(Target::Project(ProjectId::new()), local),
            vec![
                (Command::Open, "workspace_project"),
                (Command::Edit, "project_edit"),
                (Command::NewSession, "workspace_new_session"),
                (Command::Copy, "workspace_copy_path"),
                (Command::Remove, "project_remove"),
            ]
        );
    }
    assert!(
        !items(Target::Host, true)
            .iter()
            .any(|(action, _)| *action == Command::Revoke)
    );
    assert!(
        items(Target::Host, false)
            .iter()
            .any(|(action, _)| *action == Command::Revoke)
    );
    assert!(
        items(Target::Session(SessionId::new()), true)
            .contains(&(Command::Delete, "session_delete"))
    );
}

#[gpui::test]
fn captures_targets(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let project_path = directory.path().join("project");
    let repository = git2::Repository::init(&project_path).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    let tree = repository.treebuilder(None).unwrap().write().unwrap();
    let tree = repository.find_tree(tree).unwrap();
    let author = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let commit = repository
        .commit(Some("HEAD"), &author, &author, "Initial", &tree, &[])
        .unwrap();
    repository
        .branch("feature", &repository.find_commit(commit).unwrap(), false)
        .unwrap();
    let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    let local = runtime
        .block_on(Node::start(directory.path().join("local")))
        .unwrap();
    let remote = runtime
        .block_on(Node::start(directory.path().join("remote")))
        .unwrap();
    runtime
        .block_on(local.link().pair(remote.link().invite().unwrap().ticket()))
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
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        owner = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = owner.unwrap();
    wait(visual, &shell, local.id());
    for node in [&local, &remote] {
        let client = Client::new(node.local());
        let Output::Project(project) = runtime
            .block_on(client.execute(client.prepare(Request::RegisterProject {
                name: "Menu fixture".into(),
                path: project_path.to_str().unwrap().into(),
            })))
            .unwrap()
        else {
            panic!("project expected")
        };
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live_resource_action(
                    &Dispatch {
                        node: node.id(),
                        target: Target::Host,
                        command: Command::Open,
                    },
                    window,
                    cx,
                )
            })
        });
        wait(visual, &shell, node.id());
        let deadline = Instant::now() + Duration::from_secs(10);
        while !shell.read_with(visual, |shell, _| {
            shell
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .unwrap()
                .projects
                .iter()
                .any(|value| value.id == project.id)
        }) {
            assert!(Instant::now() < deadline, "project menu deadline");
            visual.run_until_parked();
            std::thread::sleep(Duration::from_millis(10));
        }
        let action = Dispatch {
            node: node.id(),
            target: Target::Project(project.id),
            command: Command::Open,
        };
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live_resource_action(&action, window, cx)
            })
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("live-overview").is_some());
        let deadline = Instant::now() + Duration::from_secs(10);
        while visual
            .debug_bounds("plugin-control-commands-terminal-new")
            .is_none()
        {
            assert!(Instant::now() < deadline, "project actions deadline");
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            std::thread::sleep(Duration::from_millis(10));
        }
        for width in [760., 1280.] {
            visual.simulate_resize(size(px(width), px(820.)));
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let viewport = visual.debug_bounds("live-overview").unwrap();
            let content = visual.debug_bounds("live-overview-content").unwrap();
            let host = visual.debug_bounds("live-overview-host").unwrap();
            assert!((content.center().x - viewport.center().x).abs() <= px(1.));
            assert!((content.top() - viewport.top() - px(40.)).abs() <= px(1.));
            let name = visual.debug_bounds("live-overview-name").unwrap();
            let path = visual.debug_bounds("live-overview-path").unwrap();
            assert!((name.left() - content.left()).abs() <= px(1.));
            assert!(host.left() > name.right());
            assert!(host.right() <= content.right());
            assert!((host.center().y - name.center().y).abs() <= px(1.));
            assert!(host.size.height < name.size.height);
            assert!((path.left() - content.left()).abs() <= px(1.));
            assert!((path.right() - content.right()).abs() <= px(1.));
            let first = visual.debug_bounds("live-new-conversation").unwrap();
            let last = visual
                .debug_bounds("plugin-control-commands-terminal-new")
                .unwrap();
            assert!((first.left() - content.left()).abs() <= px(1.));
            assert!(last.right() <= content.right());
            let records = visual.debug_bounds("project-records-list").unwrap();
            assert!(records.top() > last.bottom());
            assert!((records.left() - content.left()).abs() <= px(1.));
            assert!((records.right() - content.right()).abs() <= px(1.));
            let search = visual.debug_bounds("project-record-search").unwrap();
            let tabs = visual.debug_bounds("project-record-terminals").unwrap();
            assert!(search.left() > tabs.right());
            assert!((search.right() - records.right()).abs() <= px(1.));
            assert!((search.center().y - tabs.center().y).abs() <= px(1.));
        }
        assert!(visual.debug_bounds("live-ports").is_none());
        assert!(visual.debug_bounds("host-ports").is_none());
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live_resource_action(
                    &Dispatch {
                        node: node.id(),
                        target: Target::Host,
                        command: Command::Open,
                    },
                    window,
                    cx,
                );
            });
            window.draw(cx).clear(cx);
        });
        assert!(visual.debug_bounds("host-ports").is_some());
        let ports = visual.debug_bounds("host-ports").unwrap();
        visual.simulate_click(ports.center(), Modifiers::default());
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("port-picker").is_some());
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        visual.simulate_keystrokes("escape");
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live_resource_action(&action, window, cx);
            });
        });
        let branch = if node.id() == local.id() {
            "feature"
        } else {
            "main"
        };
        worktrees::copy_and_checkout(visual, &shell, &runtime, &action, &project_path, branch);
        worktrees::check(visual, &shell, &runtime, &action, &project_path);
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live_resource_action(
                    &Dispatch {
                        command: Command::Copy,
                        ..action.clone()
                    },
                    window,
                    cx,
                )
            })
        });
        assert_eq!(
            visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            project.path
        );
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live_resource_action(
                    &Dispatch {
                        command: Command::Edit,
                        ..action.clone()
                    },
                    window,
                    cx,
                )
            })
        });
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("project-editor").is_some());
        assert!(visual.debug_bounds("project-local").is_none());
        visual.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut last_tick = Instant::now();
        let mut advanced = Duration::ZERO;
        loop {
            // Keep Kit's lifecycle clock in step with the bounded wall-time wait.
            let now = Instant::now();
            let elapsed = now
                .saturating_duration_since(last_tick)
                .max(Duration::from_millis(10));
            last_tick = now;
            visual.executor().advance_clock(elapsed);
            advanced += elapsed;
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let notifications = visual.update(|window, cx| window.notifications(cx).len());
            if notifications == 0 {
                break;
            }
            if Instant::now() >= deadline {
                let card = visual.debug_bounds("notification-card");
                let state = visual.update(|window, cx| {
                    format!(
                        "summary={:?}, focus={:?}, mouse={:?}, active={}, hovered={}, dialog={}",
                        crate::feedback::tests::summary(window, cx),
                        window.focused(cx),
                        window.mouse_position(),
                        window.is_window_active(),
                        window.is_window_hovered(),
                        window.has_active_dialog(cx),
                    )
                });
                panic!(
                    "project notifications deadline: local={}, notifications={notifications}, advanced={advanced:?}, card={card:?}; {state}",
                    node.id() == local.id()
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        crate::live::file_tests::click(visual, "project_name-input");
        let input = visual.debug_bounds("project_name-input").unwrap();
        let notification = visual.debug_bounds("notification-card");
        let focused = visual.update(|window, cx| window.focused_input(cx));
        let click_state = visual.update(|window, cx| {
            format!(
                "input={input:?}, notification={notification:?}, notifications={}, focus={:?}, value={:?}",
                window.notifications(cx).len(),
                window.focused(cx),
                focused.as_ref().map(|input| input.value(cx))
            )
        });
        visual.simulate_keystrokes("cmd-a");
        visual.simulate_input("Edited project");
        let draft = visual.update(|window, cx| {
            (
                focused.as_ref().map(|input| input.value(cx)),
                window.focused_input(cx).map(|input| input.value(cx)),
            )
        });
        crate::live::file_tests::click(visual, "project-save");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            visual.run_until_parked();
            if shell.read_with(visual, |shell, _| {
                shell
                    .live
                    .as_ref()
                    .unwrap()
                    .view
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .projects
                    .iter()
                    .any(|p| p.id == project.id && p.name == "Edited project")
            }) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "project edit deadline: node={:?}, local={}, project={:?}, editor={}, save={}; click={click_state}, draft={draft:?}; {}",
                node.id(),
                node.id() == local.id(),
                project.id,
                visual.debug_bounds("project-editor").is_some(),
                visual.debug_bounds("project-save").is_some(),
                visual.update(|window, cx| {
                    let dialog = window.has_active_dialog(cx);
                    let feedback = window.notifications(cx).len();
                    let live = shell.read(cx).live.as_ref().unwrap();
                    let projects = live.view.snapshot.as_ref().map(|snapshot| {
                        snapshot
                            .projects
                            .iter()
                            .map(|project| (project.id, project.name.clone()))
                            .collect::<Vec<_>>()
                    });
                    format!(
                        "selected={:?}, connected={}, dialog={}, focus={:?}, projects={projects:?}, feedback={feedback:?}",
                        live.selected,
                        live.view.connected,
                        dialog,
                        window.focused(cx)
                    )
                })
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live_resource_action(
                    &Dispatch {
                        command: Command::Remove,
                        ..action.clone()
                    },
                    window,
                    cx,
                )
            })
        });
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        crate::prompts::tests::answer(visual, "project_remove");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            visual.run_until_parked();
            if shell.read_with(visual, |shell, _| {
                !shell
                    .live
                    .as_ref()
                    .unwrap()
                    .view
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .projects
                    .iter()
                    .any(|p| p.id == project.id)
            }) {
                break;
            }
            assert!(Instant::now() < deadline, "project removal deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(directory.path().is_dir());
        let other = if node.id() == local.id() {
            remote.id()
        } else {
            local.id()
        };
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live_resource_action(
                    &Dispatch {
                        node: other,
                        target: Target::Host,
                        command: Command::Open,
                    },
                    window,
                    cx,
                )
            })
        });
        wait(visual, &shell, other);
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live_resource_action(&action, window, cx)
            })
        });
        assert!(shell.read_with(visual, |shell, _| {
            shell.live.as_ref().unwrap().project.is_none()
        }));
    }
    visual.update(|window, _| window.remove_window());
    runtime.block_on(local.shutdown()).unwrap();
    runtime.block_on(remote.shutdown()).unwrap();
}
