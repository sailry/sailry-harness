use super::*;
use crate::live::{conversation::tests::wait as settled, file_tests::click};
use crate::preferences::sessions;

fn install_worktrees(fixture: &crate::activity::fixture::Fixture, index: usize, path: &str) {
    let root = std::path::Path::new(path).join("worktrees-package");
    crate::plugins::fixture::copy_package(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/worktrees"),
        &root,
    );
    let view = root.join("dev.sailry.platform/desktop/view.js");
    let source = std::fs::read_to_string(&view)
        .unwrap()
        .replace(
            "div().id('location-create-form')",
            "div().id('location-create-form').relative().child(Bounds.new('location-create-form'))",
        )
        .replace(
            "div().id(owner.pending || !owner.status ? 'location-branch-loading' : 'location-branch-name')",
            "div().id(owner.pending || !owner.status ? 'location-branch-loading' : 'location-branch-name').relative().child(Bounds.new(owner.pending || !owner.status ? 'location-branch-loading' : 'location-branch-name'))",
        )
        .replace(
            "return div().id(id).child(control);",
            "return div().id(id).relative().children(id === 'location-create-submit' ? [Bounds.new(id)] : []).child(control);",
        );
    // Transparent native probes expose the unchanged form and real button hit target.
    std::fs::write(
        view,
        format!("import {{Bounds}} from 'sailry/test';\n{source}"),
    )
    .unwrap();
    let client = Client::new(fixture.nodes[index].local());
    let execute = |command| {
        fixture
            .runtime
            .block_on(client.execute(client.prepare(command)))
            .unwrap()
    };
    let Output::Plugin(previous) = execute(Request::ReadPlugin {
        name: "worktrees".into(),
    }) else {
        panic!("Worktrees package expected")
    };
    let Output::Plugin(installed) = execute(Request::InstallPlugin {
        worktree: fixture.sessions[index].worktree,
        path: "worktrees-package".into(),
        name: "worktrees".into(),
        expected_revision: previous.summary.revision,
    }) else {
        panic!("Worktrees package expected")
    };
    assert!(installed.issues.is_empty(), "{:?}", installed.issues);
}

#[gpui::test]
fn session_actions(cx: &mut TestAppContext) {
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
        let dispatch = |command, visual: &mut VisualTestContext| {
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    shell.live_resource_action(
                        &Dispatch {
                            node,
                            target: Target::Session(session.id),
                            command,
                        },
                        window,
                        cx,
                    );
                })
            })
        };
        dispatch(Command::Rename, visual);
        click(visual, "session-name-input");
        visual.simulate_keystrokes("cmd-a");
        visual.simulate_input("Renamed conversation");
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
                .unwrap()
                .sessions
                .iter()
                .any(|s| s.id == session.id && s.activity.title == "Renamed conversation")
        });
        dispatch(Command::Copy, visual);
        assert_eq!(
            visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            "Renamed conversation"
        );
        dispatch(Command::CopyId, visual);
        assert_eq!(
            visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            session.id.to_string()
        );
        dispatch(Command::CopyPath, visual);
        let path = visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap());
        assert!(std::path::Path::new(&path).is_dir());
        install_worktrees(&fixture, index, &path);
        let repository = git2::Repository::init(&path).unwrap();
        let tree = repository.treebuilder(None).unwrap().write().unwrap();
        let author = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
        repository
            .commit(
                Some("HEAD"),
                &author,
                &author,
                "Initial",
                &repository.find_tree(tree).unwrap(),
                &[],
            )
            .unwrap();
        dispatch(Command::Open, visual);
        dispatch(Command::Pin, visual);
        dispatch(Command::MarkUnread, visual);
        settled(visual, |cx| shell.read(cx).session_unread(node, session.id));
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(
            visual.update(|_, cx| sessions::get(node, session.id, cx)),
            sessions::State { pinned: true }
        );
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-session-{}-unread", session.id).into_boxed_str()
                ))
                .is_some()
        );
        dispatch(Command::Open, visual);
        settled(visual, |cx| {
            !shell.read(cx).session_unread(node, session.id)
        });
        dispatch(Command::Unpin, visual);
        assert!(
            !visual
                .update(|_, cx| sessions::get(node, session.id, cx))
                .pinned
        );
        let before = visual.read(|cx| cx.windows());
        let saved = visual.update(|_, cx| crate::preferences::data(cx).workspaces);
        dispatch(Command::NewWindow, visual);
        let new_window = visual.read(|cx| {
            cx.windows()
                .into_iter()
                .find(|handle| !before.contains(handle))
                .expect("session window expected")
        });
        new_window
            .update(visual, |root, _, cx| {
                let root = root.downcast::<Root>().unwrap();
                let child = root.read(cx).view().clone().downcast::<Shell>().unwrap();
                assert_eq!(
                    child.read(cx).session_scope.active,
                    Key::Session(node, session.id)
                );
                assert_eq!(child.read(cx).live.as_ref().unwrap().selected, node);
                let splits = child.read(cx).splits.clone();
                splits.update(cx, |splits, cx| splits.persist(cx));
            })
            .unwrap();
        assert_eq!(
            visual.update(|_, cx| crate::preferences::data(cx).workspaces),
            saved
        );
        // Quitting from a clean secondary window must still notice drafts in the main window.
        std::fs::write(std::path::Path::new(&path).join("draft.txt"), "Saved").unwrap();
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.open_conversation_file(
                    (node, session.worktree),
                    "draft.txt".into(),
                    None,
                    window,
                    cx,
                );
            })
        });
        settled(visual, |cx| {
            matches!(&shell.read(cx).side_resource, Some(crate::resources::SideResource::Plugin(panel))
                if panel.read(cx).documents.as_ref().and_then(|documents| documents.read(cx).editor("draft.txt")).is_some())
        });
        let editor = shell.read_with(visual, |shell, cx| {
            let Some(crate::resources::SideResource::Plugin(panel)) = &shell.side_resource else {
                panic!("document panel expected");
            };
            panel
                .read(cx)
                .documents
                .as_ref()
                .unwrap()
                .read(cx)
                .editor("draft.txt")
                .unwrap()
        });
        visual.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.set_value("Unsaved", window, cx))
        });
        visual.run_until_parked();
        let mut secondary = VisualTestContext::from_window(new_window, visual);
        secondary.dispatch_action(crate::shell::Quit);
        assert_eq!(
            crate::prompts::tests::wait(&mut secondary).0,
            tr("files_unsaved").to_string()
        );
        crate::prompts::tests::answer(&mut secondary, "settings_cancel");
        assert_eq!(
            editor.read_with(visual, |editor, _| editor.value()),
            "Unsaved"
        );
        visual.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.set_value("Saved", window, cx))
        });
        visual.run_until_parked();
        new_window
            .update(visual, |_, window, _| window.remove_window())
            .unwrap();
        dispatch(Command::Fork, visual);
        settled(
            visual,
            |cx| matches!(shell.read(cx).session_scope.active, Key::Session(owner, id) if owner == node && id != session.id),
        );
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
                .any(|s| s.id != session.id && s.worktree == session.worktree)
        });
        let snapshot = shell.read_with(visual, |shell, _| {
            shell.live.as_ref().unwrap().view.snapshot.clone().unwrap()
        });
        assert!(
            snapshot
                .sessions
                .iter()
                .any(|s| s.id != session.id && s.worktree == session.worktree)
        );
        let fork = snapshot
            .sessions
            .iter()
            .find(|s| s.id != session.id && s.worktree == session.worktree)
            .unwrap()
            .id;
        dispatch(Command::Pin, visual);
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let source_row = Box::leak(format!("live-session-{}", session.id).into_boxed_str());
        let fork_row = Box::leak(format!("live-session-{fork}").into_boxed_str());
        assert!(
            visual.debug_bounds(source_row).unwrap().top()
                < visual.debug_bounds(fork_row).unwrap().top()
        );
        dispatch(Command::Unpin, visual);
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            visual.debug_bounds(fork_row).unwrap().top()
                < visual.debug_bounds(source_row).unwrap().top()
        );
        // The existing worktree form must open even when the source was not mounted.
        dispatch(Command::ForkWorktree, visual);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            visual.executor().advance_clock(Duration::from_millis(20));
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            if visual.debug_bounds("location-create-form").is_some()
                && visual.debug_bounds("location-branch-name").is_some()
            {
                break;
            }
            if Instant::now() >= deadline {
                let state = shell.read_with(visual, |shell, cx| {
                    let chat = shell.current_chat().unwrap().read(cx);
                    let panel = chat
                        .plugin_panel("worktrees", cx)
                        .map(|panel| crate::plugins::diagnostics(&panel, cx))
                        .unwrap_or_else(|| "not mounted".into());
                    format!("location={}; worktrees={panel}", chat.location_state())
                });
                panic!("worktree form deadline: {state}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        settled(visual, |cx| {
            shell.read(cx).current_chat().unwrap().read(cx).connected()
        });
        click(visual, "location-create-submit");
        settled(visual, |cx| {
            shell.read(cx).current_chat().is_some_and(|chat| {
                chat.read(cx)
                    .binding()
                    .worktree
                    .is_some_and(|tree| tree != session.worktree)
            })
        });
    }
    visual.update(|window, _| window.remove_window());
    drop(shell);
    fixture.close();
}
