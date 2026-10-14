use super::*;

#[gpui::test]
fn background_child_keeps_parent_loading(cx: &mut TestAppContext) {
    use sailry_link::CancellationToken;
    use sailry_protocol::{ProviderId, RoleId, role};
    let fixture = Fixture::new();
    let (shell, visual) = mount(cx, &fixture);
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1440.), px(940.)));
    for index in 0..2 {
        let client = Client::new(if index == 0 {
            fixture.nodes[0].local()
        } else {
            fixture.nodes[0]
                .link()
                .remote(fixture.nodes[1].link().address())
        });
        let execute = |command| {
            fixture
                .runtime
                .block_on(client.execute(client.prepare(command)))
                .unwrap()
        };
        let release = CancellationToken::new();
        let child = fixture
            .runtime
            .block_on(crate::agent_fixture::Server::markdown_held(
                "Background complete".into(),
                release.clone(),
            ));
        let parent = fixture.runtime.block_on(crate::agent_fixture::Server::tools(vec![(
            crate::agent_fixture::plugin_tool("delegation", "spawn_agent"),
            serde_json::json!({"role":"background", "task":"Finish the background task", "background":true}),
        )]));
        let original = &fixture.sessions[index];
        let Output::Snapshot(snapshot) = execute(Command::Snapshot) else {
            panic!("snapshot expected");
        };
        let mut provider = snapshot
            .providers
            .iter()
            .find(|provider| provider.id == original.config.provider)
            .unwrap()
            .clone();
        provider.endpoint = parent.endpoint.clone();
        execute(Command::PutProvider {
            expected_revision: provider.revision,
            provider: provider.clone(),
        });
        provider.id = ProviderId::new();
        provider.revision = 0;
        provider.endpoint = child.endpoint.clone();
        let Output::Provider(provider) = execute(Command::PutProvider {
            provider,
            expected_revision: 0,
        }) else {
            panic!("provider expected");
        };
        let Output::Role(role) = execute(Command::PutRole {
            role: role::Profile {
                id: RoleId::new(),
                revision: 0,
                key: "background".into(),
                name: "Background".into(),
                appearance: None,
                description: "Complete the independent task".into(),
                read_only: false,
                model: Some(role::Model {
                    provider: provider.id,
                    model: original.config.model.clone(),
                    effort: None,
                }),
                max_turns: Some(2),
                skills: vec![],
                instructions: "Complete the assigned task".into(),
            },
            expected_revision: 0,
        }) else {
            panic!("role expected");
        };
        let Output::Session(session) = execute(Command::SetSessionRoles {
            session: original.id,
            expected_revision: original.revision,
            roles: vec![role.reference()],
        }) else {
            panic!("session expected");
        };
        open(&shell, visual, &fixture, index, &session);
        let Output::QueuedTurn(turn) = execute(Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Start the independent background task and answer".into(),
        }) else {
            panic!("turn expected");
        };
        wait(visual, |cx| {
            let snapshot = shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .unwrap();
            snapshot.sessions.iter().any(|entry| {
                entry.id == session.id
                    && entry
                        .activity
                        .run
                        .as_ref()
                        .is_some_and(|run| run.turn == turn.id && run.status == Status::Completed)
            }) && snapshot.sessions.iter().any(|entry| {
                entry
                    .delegation
                    .as_ref()
                    .is_some_and(|origin| origin.session == session.id)
                    && entry
                        .activity
                        .run
                        .as_ref()
                        .is_some_and(|run| run.status == Status::Running)
            })
        });
        let loading = selector(format!("live-session-{}-loading", session.id));
        let avatar = selector(format!("header-avatar-running-{}", session.id));
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds(loading).is_some());
        assert!(visual.debug_bounds(avatar).is_some());
        assert!(
            shell.read_with(visual, |shell, _| shell.active_sessions().iter().any(
                |(node, entry)| *node == fixture.nodes[index].id() && entry.id == session.id
            ))
        );
        assert!(!child.requests.lock().unwrap().is_empty());

        // A displayed background spinner must not replace Send with the parent's Stop action.
        click(&shell, visual, "live-chat-input".into());
        visual.simulate_input("Continue while the background task runs");
        assert_eq!(
            shell.read_with(visual, |shell, cx| shell
                .current_chat()
                .unwrap()
                .read(cx)
                .draft(cx)),
            "Continue while the background task runs"
        );
        click(&shell, visual, "live-chat-send".into());
        wait(visual, |cx| {
            let snapshot = shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .unwrap();
            snapshot.sessions.iter().any(|entry| {
                entry.id == session.id
                    && entry
                        .activity
                        .run
                        .as_ref()
                        .is_some_and(|run| run.turn != turn.id && run.status == Status::Completed)
            })
        });
        let history = fixture
            .runtime
            .block_on(client.read_conversation(session.id, None, 20))
            .unwrap();
        assert_eq!(history.page.runs.len(), 2);
        assert!(
            history
                .page
                .runs
                .iter()
                .all(|run| run.status == Status::Completed)
        );
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds(loading).is_some());
        release.cancel();
        wait(visual, |cx| {
            let shell = shell.read(cx);
            let snapshot = shell.live.as_ref().unwrap().view.snapshot.as_ref().unwrap();
            let parent = snapshot
                .sessions
                .iter()
                .find(|entry| entry.id == session.id)
                .unwrap();
            parent.activity.run.as_ref().unwrap().status == Status::Completed
                && shell.session_lane(fixture.nodes[index].id(), parent)
                    != sailry_client::activity::Lane::Running
                && snapshot
                    .sessions
                    .iter()
                    .filter(|entry| {
                        entry
                            .delegation
                            .as_ref()
                            .is_some_and(|origin| origin.session == session.id)
                    })
                    .all(|entry| {
                        entry
                            .activity
                            .run
                            .as_ref()
                            .is_some_and(|run| run.status == Status::Completed)
                    })
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds(loading).is_none());
        assert!(visual.debug_bounds(avatar).is_none());
        drop(parent);
        drop(child);
    }
    visual.update(|window, _| window.remove_window());
    fixture.close();
}

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
        for (running, has_unread) in [(true, false), (true, true), (false, true), (false, false)] {
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
                    session.activity.run = running.then(|| sailry_protocol::conversation::Run {
                        worktree: session.worktree,
                        turn: sailry_protocol::TurnId::new(),
                        kind: sailry_protocol::conversation::RunKind::Task,
                        session: session.id,
                        sequence: 1,
                        revision: 1,
                        status: Status::Running,
                        error: None,
                        started_ms: Some(0),
                        finished_ms: None,
                        origin: None,
                        continuation: None,
                        input: None,
                    });
                    session.activity.attention.unread = has_unread;
                    cx.notify();
                });
                window.draw(cx).clear(cx);
            });
            assert_eq!(visual.debug_bounds(loading).is_some(), running);
            assert_eq!(visual.debug_bounds(icon).is_some(), !running);
            assert_eq!(
                visual.debug_bounds(unread).is_some(),
                !running && has_unread
            );
            if running {
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
