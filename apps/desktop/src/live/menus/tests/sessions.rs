use super::*;
use crate::live::conversation::tests::wait as settled;
use crate::live::file_tests::click;
use gpui_kit::{
    Role,
    test::{TestQueryExt as _, TestWindowExt as _},
};
use sailry_protocol::plugin::ui::Intent;

#[gpui::test]
fn archive_restore_and_delete(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let fixture = crate::activity::fixture::Fixture::new();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(fixture.services());
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        owner = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = owner.unwrap();
    for index in 0..2 {
        let node = fixture.nodes[index].id();
        let session = fixture.sessions[index].clone();
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| shell.select_live_host(node, window, cx))
        });
        wait(visual, &shell, node);
        settled(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .unwrap()
                .sessions
                .iter()
                .any(|s| s.id == session.id)
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.select_live_project(session.project.unwrap(), window, cx)
            })
        });
        let selector = |prefix: &str| {
            Box::leak(format!("{prefix}-{}", session.id).into_boxed_str()) as &'static str
        };
        let root = shell.read_with(visual, |shell, _| {
            shell
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .unwrap()
                .worktrees
                .iter()
                .find(|tree| tree.id == session.worktree)
                .unwrap()
                .path
                .clone()
        });
        git2::Repository::init(root)
            .unwrap()
            .set_head("refs/heads/sidebar-branch")
            .unwrap();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let row = visual.debug_bounds(selector("live-session")).unwrap();
        visual.simulate_mouse_move(row.center(), None, Modifiers::default());
        let branch_selector =
            Box::leak(format!("live-session-{}-branch", session.id).into_boxed_str());
        let archive_selector = selector("session-sidebar-archive");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert!(visual.debug_bounds(archive_selector).is_none());
            assert_eq!(row, visual.debug_bounds(selector("live-session")).unwrap());
            assert!(shell.read_with(visual, |shell, _| shell.sidebar.hovered
                == Some(crate::sidebar::Row::LiveSession(session.id))));
            if visual.debug_bounds(branch_selector).is_some() {
                break;
            }
            assert!(Instant::now() < deadline, "sidebar branch deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
        visual.executor().advance_clock(Duration::from_secs(1));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("tooltip-popup").is_none());
        assert!(
            super::super::session::items(false)
                .iter()
                .any(|(command, _)| *command == Command::Archive)
        );
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live_resource_action(
                    &Dispatch {
                        node,
                        target: Target::Session(session.id),
                        command: Command::Archive,
                    },
                    window,
                    cx,
                )
            })
        });

        settled(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .unwrap()
                .sessions
                .iter()
                .any(|s| s.id == session.id && s.archived)
        });
        assert!(visual.debug_bounds(selector("live-session")).is_none());
        // Project records retain the archived entry and expose the restore action.
        click(visual, selector("session-archive"));
        settled(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .unwrap()
                .sessions
                .iter()
                .any(|s| s.id == session.id && !s.archived)
        });
        assert!(visual.debug_bounds(selector("live-session")).is_some());
        click(visual, selector("session-more"));
        visual.simulate_keystrokes("escape");
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live_resource_action(
                    &Dispatch {
                        node,
                        target: Target::Session(session.id),
                        command: Command::Delete,
                    },
                    window,
                    cx,
                )
            })
        });
        assert!(visual.has_pending_prompt());
        assert!(shell.read_with(visual, |shell, _| {
            shell
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .unwrap()
                .sessions
                .iter()
                .any(|s| s.id == session.id)
        }));
        crate::prompts::tests::answer(visual, "session_delete");
        settled(visual, |cx| {
            !shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .unwrap()
                .sessions
                .iter()
                .any(|s| s.id == session.id)
        });
        assert!(visual.debug_bounds(selector("live-session")).is_none());
        let client = Client::new(fixture.nodes[index].local());
        let Output::Session(other) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Request::CreateSession {
                project: session.project,
                worktree: Some(session.worktree),
                config: Some(session.config.clone()),
            })))
            .unwrap()
        else {
            panic!("session expected")
        };
        settled(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .unwrap()
                .sessions
                .iter()
                .any(|s| s.id == other.id)
        });
        excludes_sessions(visual, &shell, node, &other);
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.reveal_session(other.clone(), window, cx)
            })
        });
        settled(visual, |cx| {
            shell
                .read(cx)
                .current_chat()
                .is_some_and(|view| view.read(cx).session() == Some(other.id))
        });
        fixture
            .runtime
            .block_on(client.execute(client.prepare(Request::RemoveSession {
                session: other.id,
                expected_revision: other.revision,
            })))
            .unwrap();
        settled(visual, |cx| {
            !shell
                .read(cx)
                .session_scope
                .open
                .contains(&Key::Session(node, other.id))
                && !shell.read(cx).chats.views.contains_key(&(node, other.id))
        });
    }
    visual.update(|window, _| window.remove_window());
    drop(shell);
    fixture.close();
}

fn excludes_sessions(
    visual: &mut VisualTestContext,
    shell: &Entity<Shell>,
    node: NodeId,
    session: &sailry_protocol::Session,
) {
    for (command, intent, package) in [
        (Command::Branches, Intent::GitBranches, "git"),
        (Command::Worktrees, Intent::Worktrees, "worktrees"),
    ] {
        let registry = visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .project_plugins(session.project.unwrap(), window, cx)
                    .unwrap()
            })
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live_resource_action(
                    &Dispatch {
                        node,
                        target: Target::Project(session.project.unwrap()),
                        command,
                    },
                    window,
                    cx,
                );
            })
        });
        surface(visual, &registry, intent, package, false);
        visual.simulate_input(&crate::activity::title(session));
        surface(visual, &registry, intent, package, true);
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
    }
}

#[track_caller]
fn surface(
    visual: &mut VisualTestContext,
    registry: &Entity<crate::plugins::contributions::Registry>,
    intent: Intent,
    package: &str,
    empty: bool,
) {
    let prefix = match package {
        "git" => "git-picker-",
        "worktrees" => "worktrees-",
        _ => panic!("unexpected manager package: {package}"),
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        // Contribution readiness and Kit search completion both use executor timers.
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        let (rows, inputs, focused) = visual.update(|window, cx| {
            window.render_frame(cx);
            let rows = window
                .elements()
                .into_iter()
                .filter(|row| {
                    matches!(row.path().last(), Some(ElementId::NamedInteger(id, 0))
                        if id.starts_with(prefix)
                            && row.path()[..row.path().len() - 1].contains(&ElementId::Name(id.clone())))
                        && row.visible()
                })
                .collect::<Vec<_>>();
            let inputs = rows
                .first()
                .and_then(|row| match row.path().last() {
                    Some(ElementId::NamedInteger(id, _)) => Some(id.clone()),
                    _ => None,
                })
                .map(|id| {
                    window
                        .within(id)
                        .find_all_by_role(Role::TextInput)
                        .into_iter()
                        .filter(|input| input.visible())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let focused = inputs.len() == 1 && inputs[0].focused() == Some(true);
            (rows, inputs, focused)
        });
        let found = if empty {
            visual.debug_bounds("plugin-picker-empty").is_some() && rows.is_empty()
        } else {
            rows.len() == 1 && focused
        };
        if found {
            return;
        }
        if Instant::now() >= deadline {
            let state = visual.update(|_, cx| {
                let registry = registry.read(cx);
                let panel = registry
                    .test_panel(package)
                    .map(|panel| crate::plugins::diagnostics(&panel, cx))
                    .unwrap_or_else(|| "package contribution is not mounted".to_owned());
                format!(
                    "ready={}, intent={}, {panel}",
                    registry.ready(cx),
                    registry.intent(intent, cx).is_some(),
                )
            });
            panic!(
                "manager surface deadline: {package}, empty={empty}, focused={focused}, rows={rows:?}, inputs={inputs:?}; {state}"
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
