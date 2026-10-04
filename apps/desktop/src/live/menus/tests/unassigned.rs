use super::*;
use crate::activity::fixture::Fixture;
use crate::live::{conversation::tests::wait as settled, file_tests::click};

fn mount<'a>(
    fixture: &Fixture,
    cx: &'a mut TestAppContext,
) -> (Entity<Shell>, &'a mut VisualTestContext) {
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        owner = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = owner.unwrap();
    wait(visual, &shell, fixture.nodes[0].id());
    (shell, visual)
}

fn init(fixture: &Fixture, cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(fixture.services());
    });
}

fn create(fixture: &Fixture, client: &Client, index: usize) -> sailry_protocol::Session {
    let Output::Session(session) = fixture
        .runtime
        .block_on(client.execute(client.prepare(Request::CreateSession {
            project: None,
            worktree: None,
            config: Some(fixture.sessions[index].config.clone()),
        })))
        .unwrap()
    else {
        panic!("session expected")
    };
    assert!(session.project.is_none());
    session
}

fn open_menu(
    visual: &mut VisualTestContext,
    selector: &'static str,
    node: NodeId,
    target: Target,
) -> Vec<(Dispatch, bool)> {
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    visual.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.update(|window, cx| capture::record(window, cx, Vec::new()));
    visual.simulate_mouse_down(bounds.center(), MouseButton::Right, Modifiers::default());
    visual.simulate_mouse_up(bounds.center(), MouseButton::Right, Modifiers::default());
    visual.run_until_parked();
    assert!(
        visual.update(|window, cx| capture::opened(window, cx, node, target)),
        "native resource menu target changed"
    );
    visual.update(|window, cx| capture::actions(window, cx))
}

#[gpui::test]
fn new_session_preserves_the_draft(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    init(&fixture, cx);
    for index in 0..2 {
        let (shell, visual) = mount(&fixture, cx);
        let node = fixture.nodes[index].id();
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| shell.select_live_host(node, window, cx))
        });
        wait(visual, &shell, node);
        let client = shell.read_with(visual, |shell, _| shell.live.as_ref().unwrap().client());
        let before = fixture
            .runtime
            .block_on(client.execute(client.prepare(Request::Snapshot)))
            .unwrap();
        click(visual, "unassigned-add");
        settled(visual, |cx| {
            shell.read(cx).current_chat().is_some_and(|view| {
                let view = view.read(cx);
                view.connected()
                    && view.binding().client.target() == node
                    && view.binding().project.is_none()
                    && view.binding().worktree.is_none()
            })
        });
        assert_eq!(
            fixture
                .runtime
                .block_on(client.execute(client.prepare(Request::Snapshot)))
                .unwrap(),
            before,
            "opening an empty-group draft must not create a session"
        );
        let session = create(&fixture, &client, index);
        settled(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot.sessions.iter().any(|entry| entry.id == session.id)
                })
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.select_live_project(fixture.sessions[index].project.unwrap(), window, cx);
                shell.new_live_conversation(window, cx);
            })
        });
        settled(visual, |cx| {
            shell.read(cx).current_chat().is_some_and(|view| {
                view.read(cx).connected()
                    && view.read(cx).binding().project == fixture.sessions[index].project
            })
        });
        click(visual, "live-chat-input");
        visual.simulate_input("Keep this draft 中文");
        let draft = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
        let attachment = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(attachment.path(), "Preserved attachment").unwrap();
        click(visual, "live-attach");
        assert!(visual.did_prompt_for_paths());
        visual.simulate_path_prompt_response(|options| {
            assert!(options.files && !options.directories && options.multiple);
            Some(vec![attachment.path().to_path_buf()])
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            if visual.debug_bounds("attachment-card-0").is_some()
                && draft.read_with(visual, |view, _| view.can_retarget())
            {
                break;
            }
            assert!(Instant::now() < deadline, "draft attachment deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
        let before = fixture
            .runtime
            .block_on(client.execute(client.prepare(Request::Snapshot)))
            .unwrap();
        click(visual, "projects-heading");
        assert!(visual.debug_bounds("unassigned-sessions").is_some());
        let project = Box::leak(
            format!("live-project-{}", fixture.sessions[index].project.unwrap()).into_boxed_str(),
        );
        assert!(visual.debug_bounds(project).is_none());
        click(visual, "unassigned-add");
        settled(visual, |cx| {
            let view = draft.read(cx);
            view.connected()
                && view.binding().client.target() == node
                && view.binding().project.is_none()
                && view.binding().worktree.is_none()
        });
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone()),
            draft
        );
        assert_eq!(
            draft.read_with(visual, |view, cx| view.draft(cx)),
            "Keep this draft 中文"
        );
        assert!(visual.debug_bounds("attachment-card-1").is_some());
        assert!(visual.debug_bounds("attachment-card-0").is_none());
        assert_eq!(
            fixture
                .runtime
                .block_on(client.execute(client.prepare(Request::Snapshot)))
                .unwrap(),
            before,
            "opening a draft must not create a session"
        );
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.select_live_host(fixture.nodes[1 - index].id(), window, cx)
            });
            shell.update(cx, |shell, cx| {
                shell.new_unassigned_conversation(node, window, cx)
            });
        });
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.live.as_ref().unwrap().selected),
            fixture.nodes[1 - index].id()
        );
        assert_eq!(
            draft.read_with(visual, |view, _| view.binding().client.target()),
            node
        );
        visual.update(|window, _| window.remove_window());
    }
    fixture.close();
}

#[gpui::test]
fn session_actions_without_a_project(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    init(&fixture, cx);
    for index in 0..2 {
        let (shell, visual) = mount(&fixture, cx);
        let node = fixture.nodes[index].id();
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| shell.select_live_host(node, window, cx))
        });
        wait(visual, &shell, node);
        let client = shell.read_with(visual, |shell, _| shell.live.as_ref().unwrap().client());
        let session = create(&fixture, &client, index);
        settled(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot.sessions.iter().any(|entry| entry.id == session.id)
                })
        });
        // Only the presentation snapshot omits the tree; the Node and session remain intact.
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .live
                    .as_mut()
                    .unwrap()
                    .view
                    .snapshot
                    .as_mut()
                    .unwrap()
                    .worktrees
                    .retain(|tree| tree.id != session.worktree);
                cx.notify();
            })
        });
        let selector = Box::leak(format!("live-session-{}", session.id).into_boxed_str());
        let actions = open_menu(visual, selector, node, Target::Session(session.id));
        for command in [
            Command::CopyPath,
            Command::OpenDirectory,
            Command::Fork,
            Command::ForkWorktree,
        ] {
            assert!(
                actions
                    .iter()
                    .any(|(action, disabled)| action.command == command && *disabled)
            );
        }
        for command in [
            Command::Rename,
            Command::Pin,
            Command::Archive,
            Command::Delete,
            Command::Copy,
            Command::CopyId,
            Command::NewWindow,
        ] {
            assert!(
                actions
                    .iter()
                    .any(|(action, disabled)| action.command == command && !disabled)
            );
        }
        visual.update(|window, cx| capture::choose(Command::CopyId, window, cx));
        visual.run_until_parked();
        assert_eq!(
            visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            session.id.to_string()
        );
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.select_live_host(fixture.nodes[1 - index].id(), window, cx)
            });
            cx.write_to_clipboard(ClipboardItem::new_string("Unchanged".into()));
            capture::choose(Command::CopyId, window, cx);
        });
        visual.run_until_parked();
        assert_eq!(
            visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            "Unchanged"
        );
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| shell.select_live_host(node, window, cx))
        });
        wait(visual, &shell, node);
        open_menu(visual, selector, node, Target::Session(session.id));
        visual.update(|window, cx| capture::choose(Command::Rename, window, cx));
        click(visual, "session-name-input");
        visual.simulate_keystrokes("cmd-a");
        visual.simulate_input("Renamed unassigned");
        click(visual, "session-rename-save");
        settled(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot.sessions.iter().any(|entry| {
                        entry.id == session.id && entry.activity.title == "Renamed unassigned"
                    })
                })
        });
        open_menu(visual, selector, node, Target::Session(session.id));
        visual.update(|window, cx| capture::choose(Command::Archive, window, cx));
        settled(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .sessions
                        .iter()
                        .any(|entry| entry.id == session.id && entry.archived)
                })
        });
        let removed = create(&fixture, &client, index);
        settled(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot.sessions.iter().any(|entry| entry.id == removed.id)
                })
        });
        let selector = Box::leak(format!("live-session-{}", removed.id).into_boxed_str());
        open_menu(visual, selector, node, Target::Session(removed.id));
        visual.update(|window, cx| capture::choose(Command::Delete, window, cx));
        assert!(visual.has_pending_prompt());
        crate::prompts::tests::answer(visual, "session_delete");
        settled(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    !snapshot.sessions.iter().any(|entry| entry.id == removed.id)
                })
        });
        visual.update(|window, _| window.remove_window());
    }
    fixture.close();
}
