use super::*;

#[gpui::test]
fn worktree_and_loading(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (shell, visual) = mount(cx, &fixture);
    for index in 0..2 {
        let original = &fixture.sessions[index];
        open(&shell, visual, &fixture, index, original);
        let client = shell.read_with(visual, |shell, _| shell.live.as_ref().unwrap().client());
        let root = shell.read_with(visual, |shell, _| {
            shell
                .live
                .as_ref()
                .unwrap()
                .selected_worktree()
                .unwrap()
                .path
                .clone()
        });
        let repository = git2::Repository::init(&root).unwrap();
        let tree = repository.index().unwrap().write_tree().unwrap();
        let signature = git2::Signature::now("Fixture", "fixture@example.test").unwrap();
        let commit = repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "Initial fixture",
                &repository.find_tree(tree).unwrap(),
                &[],
            )
            .unwrap();
        let path = std::path::Path::new(&root).with_file_name(format!("worktree-{index}"));
        let Output::Worktree(tree) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::CreateWorktree {
                project: original.project.unwrap(),
                path: path.to_str().unwrap().into(),
                branch: "sidebar-icons".into(),
                commit: commit.to_string(),
            })))
            .unwrap()
        else {
            panic!("worktree expected")
        };
        let Output::Session(session) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::CreateSession {
                project: original.project,
                worktree: Some(tree.id),
                config: Some(original.config.clone()),
            })))
            .unwrap()
        else {
            panic!("session expected")
        };
        open(&shell, visual, &fixture, index, &session);
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            visual
                .debug_bounds(selector(format!("live-session-{}-worktree", original.id)))
                .is_none()
        );
        let row = selector(format!("live-session-{}", session.id));
        let label = selector(format!("live-session-{}-label", session.id));
        let icon = selector(format!("live-session-{}-worktree", session.id));
        let loading = selector(format!("live-session-{}-loading", session.id));
        let unread = selector(format!("live-session-{}-unread", session.id));
        let title_bounds = visual.debug_bounds(label).unwrap();
        let icon_bounds = visual.debug_bounds(icon).unwrap();
        assert!(icon_bounds.left() > title_bounds.right());
        assert!((icon_bounds.center().y - title_bounds.center().y).abs() < px(1.));
        assert!(visual.debug_bounds(loading).is_none());
        let branch = hover(visual, row);
        let hovered_title = visual.debug_bounds(label).unwrap();
        assert_eq!(hovered_title.left(), title_bounds.left());
        assert!(hovered_title.right() < branch.left());
        assert!(branch.right() < icon_bounds.left());
        assert_eq!(visual.debug_bounds(icon), Some(icon_bounds));

        // Inject Client projection states to exercise presentation priority without a model run.
        for (queued, has_unread) in [(1, false), (1, true), (0, true), (0, false)] {
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    let session = shell
                        .live
                        .as_mut()
                        .unwrap()
                        .view
                        .snapshot
                        .as_mut()
                        .unwrap()
                        .sessions
                        .iter_mut()
                        .find(|entry| entry.id == session.id)
                        .unwrap();
                    session.activity.queued = queued;
                    session.activity.attention.unread = has_unread;
                    cx.notify();
                });
                window.draw(cx).clear(cx);
            });
            assert_eq!(visual.debug_bounds(loading).is_some(), queued > 0);
            assert_eq!(visual.debug_bounds(icon).is_some(), queued == 0);
            assert_eq!(
                visual.debug_bounds(unread).is_some(),
                queued == 0 && has_unread
            );
            if queued > 0 {
                assert_eq!(visual.debug_bounds(loading), Some(icon_bounds));
                assert_eq!(visual.debug_bounds(label), Some(hovered_title));
            }
        }
        click(&shell, visual, row.into());
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.session_scope.active),
            Key::Session(fixture.nodes[index].id(), session.id)
        );
    }
    visual.update(|window, _| window.remove_window());
    fixture.close();
}
