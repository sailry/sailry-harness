use super::*;

#[gpui::test]
fn sends_without_project_locally_and_remotely(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        for attachment in [false, true] {
            eprintln!("unassigned send: remote={remote}, attachment={attachment}");
            let fixture = fixture::Fixture::with_tools(remote, Vec::new());
            let mut binding = fixture.binding.clone();
            binding.project = None;
            binding.worktree = None;
            binding.project_name = SharedString::default();
            binding.branch = SharedString::default();
            let (view, visual) = fixture::open_session(cx, binding, None);
            wait(visual, |cx| view.read(cx).connected());
            visual.update(|window, cx| {
                view.update(cx, |view, cx| {
                    view.config = Some(fixture.session.config.clone());
                    if attachment {
                        let path = fixture.directory.path().join("note.txt");
                        std::fs::write(&path, "Attachment fixture").unwrap();
                        view.attach_paths(vec![path], window, cx);
                    }
                    cx.notify();
                })
            });
            click(visual, "live-chat-input");
            visual.simulate_input("A conversation without a project");
            click(visual, "live-chat-send");
            wait(visual, |cx| {
                let current = view.read(cx);
                assert!(current.error.is_none(), "send failed: {:?}", current.error);
                if let Some(history) = &current.history.snapshot {
                    for run in &history.page.runs {
                        assert_ne!(run.status, Status::Failed, "run failed: {:?}", run.error);
                    }
                }
                view.read(cx)
                    .history
                    .snapshot
                    .as_ref()
                    .is_some_and(|history| {
                        history
                            .page
                            .runs
                            .iter()
                            .any(|run| run.status == Status::Completed)
                    })
            });
            let session = view.read_with(visual, |view, _| {
                assert!(view.error.is_none());
                assert!(view.binding.project.is_none());
                assert!(!view.git);
                assert!(view.binding.branch.is_empty());
                view.session.clone().unwrap()
            });
            assert_ne!(session.worktree, fixture.session.worktree);
            wait(visual, |cx| {
                view.read(cx)
                    .node
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| {
                        snapshot
                            .worktrees
                            .iter()
                            .any(|tree| tree.id == session.worktree)
                    })
            });
            assert!(visual.debug_bounds("composer-current-branch").is_none());
            view.read_with(visual, |view, _| {
                let tree = view
                    .node
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .worktrees
                    .iter()
                    .find(|tree| tree.id == session.worktree)
                    .unwrap();
                assert!(tree.project.is_none());
                assert_eq!(
                    std::path::Path::new(&tree.path),
                    fixture
                        .directory
                        .path()
                        .join("node/workspaces/sessions")
                        .join(session.id.to_string())
                        .canonicalize()
                        .unwrap()
                );
            });
            if attachment {
                assert!(
                    fixture
                        .server
                        .requests
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|request| request.to_string().contains("Attachment fixture"))
                );
            }
            visual.update(|window, _| window.remove_window());
            fixture.close();
        }
    }
}

#[gpui::test]
fn project_choice_preserves_draft(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, Vec::new());
        let (view, visual) = fixture::open_session(cx, fixture.binding.clone(), None);
        wait(visual, |cx| view.read(cx).connected());
        click(visual, "live-chat-input");
        visual.simulate_input("Keep this draft");
        visual.update(|window, cx| {
            view.update(cx, |view, cx| view.choose_location(true, window, cx))
        });
        fixture::tap(visual, "location-unassigned");
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).binding.project.is_none()
        });
        view.read_with(visual, |view, cx| {
            assert_eq!(view.draft(cx), "Keep this draft");
            assert!(view.binding.worktree.is_none());
        });
        visual.update(|window, cx| {
            view.update(cx, |view, cx| view.choose_location(true, window, cx))
        });
        fixture::tap(
            visual,
            &format!("location-project-{}", fixture.binding.project.unwrap()),
        );
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).binding.project == fixture.binding.project
        });
        assert_eq!(
            view.read_with(visual, |view, cx| view.draft(cx)),
            "Keep this draft"
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
