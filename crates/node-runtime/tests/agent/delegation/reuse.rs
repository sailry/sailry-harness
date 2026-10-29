use super::fixture::Fixture;
use super::*;

mod approvals;

#[tokio::test]
async fn omits_previous_answer_from_current_outcome() {
    for remote in [false, true] {
        let parent = Server::scripted_tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"role":"review","task":"Initial child task"}),
        )])
        .await;
        let child = Server::reasoning_after_answer().await;
        let fixture = Fixture::start(remote, parent, child).await;
        let first = fixture.push("Read the initial child answer").await;
        let original = finished(&fixture.client, fixture.session.id, first.id).await;
        assert_eq!(original.runs[0].status, Status::Completed);
        let child = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        let before = history(&fixture.client, child.id).await;
        assert!(text(&before).contains("answer-fixture-child"));
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        fixture.parent.replace_tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"session":child.id,"task":"Continue with reasoning only"}),
        )]);
        let second = fixture.push("Continue the existing child").await;
        let parent = finished(&fixture.client, fixture.session.id, second.id).await;
        assert!(
            parent
                .runs
                .iter()
                .all(|run| run.status == Status::Completed)
        );
        let after = history(&fixture.client, child.id).await;
        assert_eq!(after.runs.len(), 2);
        assert!(after.runs.iter().all(|run| run.status == Status::Completed));
        let current = after.runs.last().unwrap().turn;
        let parts = after
            .entries
            .iter()
            .filter(|entry| entry.turn == current && entry.author == "assistant")
            .flat_map(|entry| &entry.parts)
            .collect::<Vec<_>>();
        assert!(parts.iter().any(|part| matches!(part, Part::Thinking(_))));
        assert!(parts.iter().all(|part| !matches!(part, Part::Text(_))));
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 2);
        let requests = fixture.parent.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 4);
        let outcome = requests.last().unwrap()["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .filter(|message| message["role"] == "user")
            .filter_map(|message| message["content"].as_str())
            .find(|text| text.starts_with("Subagent ") && text.contains(" completed:"))
            .unwrap();
        assert!(outcome.contains(&child.id.to_string()));
        assert!(!outcome.contains("answer-fixture-child"));
        assert!(
            requests[2]["messages"]
                .to_string()
                .contains("answer-fixture-child")
        );
        fixture.close().await;
    }
}

#[tokio::test]
async fn preserves_child_history_and_configuration() {
    for remote in [false, true] {
        let parent = Server::turn_tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"role":"review","task":"Initial child task"}),
        )])
        .await;
        let child = Server::tools(vec![(
            plugin_tool("files", "read_file"),
            json!({"path":"source.txt"}),
        )])
        .await;
        let mut fixture = Fixture::start(remote, parent, child).await;
        std::fs::write(
            fixture.root().join("source.txt"),
            "Prior child evidence 中文 🙂",
        )
        .unwrap();
        let first = fixture.push("First parent-only private context").await;
        let original = finished(&fixture.client, fixture.session.id, first.id).await;
        assert_eq!(original.runs[0].status, Status::Completed);
        assert_eq!(original.children.len(), 1);
        let child = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        let child_page = history(&fixture.client, child.id).await;
        assert_eq!(child_page.runs.len(), 1);
        assert_eq!(child_page.runs[0].status, Status::Completed);
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 2);
        let next_parent = Server::turn_tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"session":child.id,"task":"Continue the same child task"}),
        )])
        .await;
        fixture.route_parent(&next_parent.endpoint).await;
        let mut provider = fixture
            .snapshot()
            .await
            .providers
            .into_iter()
            .find(|provider| provider.id == fixture.session.config.provider)
            .unwrap();
        let revision = provider.revision;
        provider.models.push(model("fixture-new-parent"));
        provider.default_model = "fixture-new-parent".into();
        fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                provider,
                expected_revision: revision,
            }))
            .await
            .unwrap();
        let mut config = fixture.session.config.clone();
        config.model = "fixture-new-parent".into();
        config.effort = Effort::Low;
        let Output::Session(session) = fixture
            .client
            .execute(fixture.client.prepare(Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                config,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let mut role = fixture.role.clone();
        role.key = "changed-review".into();
        role.name = "Changed review".into();
        role.instructions = "Changed role instructions".into();
        role.max_turns = Some(1);
        let Output::Role(role) = fixture
            .client
            .execute(fixture.client.prepare(Command::PutRole {
                role,
                expected_revision: fixture.role.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("role expected")
        };
        let Output::Session(session) = fixture
            .client
            .execute(fixture.client.prepare(Command::SetSessionRoles {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                roles: vec![role.reference()],
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        fixture.session = session;
        fixture.provider.endpoint = "http://127.0.0.1:1/v1".into();
        fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                provider: fixture.provider.clone(),
                expected_revision: fixture.provider.revision,
            }))
            .await
            .unwrap();
        disable_tools(&fixture.client, &["files"]).await;
        let request = fixture.client.prepare(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Second parent-only private context".into(),
        });
        let Output::QueuedTurn(second) = fixture.client.execute(request.clone()).await.unwrap()
        else {
            panic!("turn expected")
        };
        let parent_page = finished(&fixture.client, fixture.session.id, second.id).await;
        assert!(
            parent_page
                .runs
                .iter()
                .all(|run| run.status == Status::Completed)
        );
        assert_eq!(parent_page.children.len(), 2);
        assert_eq!(parent_page.children[0], original.children[0]);
        let reused = &parent_page.children[1];
        assert_eq!(reused.run.session, child.id);
        assert_ne!(reused.run.turn, child_page.runs[0].turn);
        assert_eq!(reused.run.worktree, child.worktree);
        assert_eq!(reused.run.revision, child.revision);
        assert_eq!(reused.run.status, Status::Completed);
        assert_eq!(reused.name.as_deref(), Some("Review"));
        assert_eq!(reused.origin.session, fixture.session.id);
        assert_eq!(reused.origin.turn, second.id);
        assert_ne!(reused.origin.entry, original.children[0].origin.entry);
        assert_eq!(reused.origin.role, child.delegation.as_ref().unwrap().role);
        let retained = history(&fixture.client, child.id).await;
        assert_eq!(retained.runs.len(), 2);
        assert!(retained.entries.starts_with(&child_page.entries));
        assert!(text(&retained).contains("Initial child task"));
        assert!(text(&retained).contains("Continue the same child task"));
        let snapshot = fixture.snapshot().await;
        let current = snapshot
            .sessions
            .iter()
            .find(|session| session.id == child.id)
            .unwrap();
        assert_eq!(current.project, child.project);
        assert_eq!(current.worktree, child.worktree);
        assert_eq!(current.revision, child.revision);
        assert_eq!(current.config, child.config);
        assert_eq!(current.roles, child.roles);
        assert_eq!(current.profile, child.profile);
        assert_eq!(current.delegation, child.delegation);
        assert_eq!(
            snapshot
                .sessions
                .iter()
                .filter(|session| session
                    .delegation
                    .as_ref()
                    .is_some_and(|origin| origin.session == fixture.session.id))
                .count(),
            1
        );
        let mut admitted = Vec::new();
        for turn in snapshot
            .turns
            .iter()
            .filter(|turn| turn.session == child.id)
        {
            admitted.push(
                fixture
                    .client
                    .read_turn_capture(child.id, turn.id)
                    .await
                    .unwrap(),
            );
        }
        assert_eq!(admitted.len(), 2);
        assert_eq!(admitted[0].config, admitted[1].config);
        assert!(
            admitted[0]
                .plugins
                .iter()
                .any(|reference| reference.name == "files")
        );
        assert!(
            !admitted[1]
                .plugins
                .iter()
                .any(|reference| reference.name == "files")
        );
        let requests = fixture.child.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 3);
        let resumed = &requests[2];
        assert_eq!(resumed["model"], "fixture-child");
        let context = resumed["messages"].to_string();
        assert!(context.contains("Frozen review instructions"));
        assert!(context.contains("Initial child task"));
        assert!(context.contains("answer-fixture-child"));
        assert!(context.contains("Prior child evidence"));
        assert!(context.contains("Continue the same child task"));
        assert!(!context.contains("Changed role instructions"));
        assert!(!context.contains("parent-only private context"));
        assert!(
            !resumed["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["function"]["name"] == "read_file")
        );
        assert_eq!(fixture.parent.requests.lock().unwrap().len(), 2);
        assert_eq!(next_parent.requests.lock().unwrap().len(), 2);
        assert_eq!(
            fixture.client.execute(request.clone()).await.unwrap(),
            Output::QueuedTurn(second.clone())
        );
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 3);
        let Output::Session(branch) = fixture
            .client
            .execute(fixture.client.prepare(Command::ForkConversation {
                session: fixture.session.id,
                through: second.id,
                expected_revision: fixture.session.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("fork expected")
        };
        let fork = history(&fixture.client, branch.id).await;
        assert_eq!(fork.children, parent_page.children);
        let mut stream = fixture
            .client
            .subscribe_conversation(branch.id)
            .await
            .unwrap();
        let mut projection =
            sailry_client::conversation::Projection::new(fixture.client.target(), branch.id, 0);
        projection.apply(0, stream.next().await.unwrap()).unwrap();
        assert_eq!(
            projection.snapshot().unwrap().page.children,
            parent_page.children
        );
        drop(stream);
        let Output::Rewound(rewound) = fixture
            .client
            .execute(fixture.client.prepare(Command::RewindConversation {
                session: fixture.session.id,
                through: Some(first.id),
                expected_head: second.id,
                expected_revision: parent_page.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("rewind expected")
        };
        let trimmed = history(&fixture.client, fixture.session.id).await;
        assert!(rewound.backup.is_none());
        assert_eq!(trimmed.children, original.children);
        let backup = history(&fixture.client, branch.id).await;
        assert_eq!(backup.children, parent_page.children);
        let fixture = fixture.restart().await;
        assert_eq!(
            history(&fixture.client, child.id).await.entries,
            retained.entries
        );
        assert_eq!(
            history(&fixture.client, fixture.session.id).await.children,
            original.children
        );
        assert_eq!(
            history(&fixture.client, branch.id).await.children,
            parent_page.children
        );
        assert_eq!(
            fixture.client.execute(request).await.unwrap(),
            Output::QueuedTurn(second)
        );
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 3);
        assert_eq!(
            fixture
                .snapshot()
                .await
                .turns
                .iter()
                .filter(|turn| turn.session == child.id)
                .count(),
            2
        );
        fixture.close().await;
    }
}
