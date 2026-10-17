use super::*;

#[tokio::test]
async fn reuses_registered_sessions_and_configuration() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote, models::Server::start(false).await, false).await;
        let mut editor = fixture.editor().await;
        editor.initialize().await;
        let listed = editor
            .request(2, "session/list", json!({"cwd":fixture.root()}), None)
            .await;
        assert!(
            listed["result"]["sessions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|session| session["sessionId"] == json!(fixture.session.id))
        );
        let request = RequestId::new();
        let created = editor.request(3, "session/new", json!({"cwd":fixture.root(),"mcpServers":[],"_meta":{"sailry":{"request_id":request}}}), None).await;
        let id: SessionId = serde_json::from_value(created["result"]["sessionId"].clone()).unwrap();
        let Output::Session(session) = fixture.execute(Command::ReadSession { session: id }).await
        else {
            panic!("session expected")
        };
        assert_eq!(session.config, fixture.session.config);
        assert_eq!(session.worktree, fixture.session.worktree);
        let revision = session.revision;
        let changed = editor
            .request(
                4,
                "session/set_mode",
                json!({"sessionId":id,"modeId":"plan"}),
                None,
            )
            .await;
        let Output::Session(session) = fixture.execute(Command::ReadSession { session: id }).await
        else {
            panic!("session expected")
        };
        assert_eq!(session.config.mode, WorkMode::Plan);
        assert_eq!(session.config.permission, fixture.session.config.permission);
        assert_eq!(session.config.credential, fixture.session.config.credential);
        assert_eq!(
            changed["result"]["_meta"]["sailry"]["revision"],
            session.revision
        );
        assert!(session.revision > revision);
        let changed = editor
            .request(
                5,
                "session/set_config_option",
                json!({"sessionId":id,"configId":"mode","value":"code"}),
                None,
            )
            .await;
        assert!(changed.get("result").is_some(), "{changed}");
        let resumed = editor
            .request(
                6,
                "session/resume",
                json!({"sessionId":id,"cwd":fixture.root(),"mcpServers":[]}),
                None,
            )
            .await;
        assert!(resumed.get("result").is_some(), "{resumed}");
        let empty_fork = editor
            .request(
                7,
                "session/fork",
                json!({"sessionId":id,"cwd":fixture.root(),"mcpServers":[]}),
                None,
            )
            .await;
        assert!(empty_fork.get("error").is_some());
        let original = editor.load(&fixture).await;
        assert!(original.get("result").is_some());
        assert!(
            editor
                .request(
                    8,
                    "session/prompt",
                    prompt(
                        fixture.session.id,
                        RequestId::new(),
                        fixture.session.revision,
                        "Make a forkable turn"
                    ),
                    None
                )
                .await
                .get("result")
                .is_some()
        );
        let forked = editor
            .request(
                9,
                "session/fork",
                json!({"sessionId":fixture.session.id,"cwd":fixture.root(),"mcpServers":[]}),
                None,
            )
            .await;
        let fork: SessionId =
            serde_json::from_value(forked["result"]["sessionId"].clone()).unwrap();
        assert_ne!(fork, fixture.session.id);
        let Output::Session(fork) = fixture
            .execute(Command::ReadSession { session: fork })
            .await
        else {
            panic!("session expected")
        };
        assert_eq!(fork.config, fixture.session.config);
        assert!(fork.fork.is_some());
        let canonical = fixture
            .client
            .read_conversation(fork.id, None, 100)
            .await
            .unwrap();
        assert_eq!(
            canonical.page.entries.len(),
            fixture.page().await.entries.len()
        );
        assert_eq!(fixture.model.requests.lock().unwrap().len(), 1);
        assert!(
            editor
                .request(10, "session/close", json!({"sessionId":fork.id}), None)
                .await
                .get("result")
                .is_some()
        );
        assert!(matches!(
            fixture
                .execute(Command::ReadSession { session: fork.id })
                .await,
            Output::Session(_)
        ));
        assert!(
            sailry_node_runtime::Node::start(fixture.node.profile())
                .await
                .is_err()
        );
        editor.close().await;
        fixture.close().await;
    }
}

#[tokio::test]
async fn rejects_unregistered_workspaces_and_installation() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote, models::Server::start(false).await, false).await;
        let mut editor = fixture.editor().await;
        editor.initialize().await;
        for (id, params) in [
            (
                2,
                json!({"cwd":fixture.directory.path().join("unknown"),"mcpServers":[]}),
            ),
            (
                3,
                json!({"cwd":fixture.root(),"mcpServers":[{"name":"unexpected","command":"false","args":[],"env":[]}]}),
            ),
            (
                4,
                json!({"cwd":fixture.root(),"additionalDirectories":[fixture.directory.path()],"mcpServers":[]}),
            ),
        ] {
            let rejected = editor.request(id, "session/new", params, None).await;
            assert!(rejected.get("error").is_some(), "{rejected}");
        }
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.sessions.len(), 1);
        assert!(fixture.model.requests.lock().unwrap().is_empty());
        assert!(!fixture.directory.path().join("unknown").exists());
        editor.close().await;
        fixture.close().await;
    }
}

#[tokio::test]
async fn retains_revision_conflicts_from_other_controllers() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote, models::Server::start(false).await, false).await;
        let mut editor = fixture.editor().await;
        editor.initialize().await;
        editor.load(&fixture).await;
        let mut config = fixture.session.config.clone();
        config.mode = WorkMode::Plan;
        config.permission = sailry_protocol::Permission::Full;
        let Output::Session(changed) = fixture
            .execute(Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                config,
            })
            .await
        else {
            panic!("session expected")
        };
        let rejected = editor
            .request(
                3,
                "session/set_mode",
                json!({"sessionId":fixture.session.id,"modeId":"code"}),
                None,
            )
            .await;
        assert_eq!(
            rejected["error"]["data"]["sailry"]["code"],
            "revision_conflict"
        );
        let accepted = editor
            .request(
                4,
                "session/set_mode",
                json!({"sessionId":fixture.session.id,"modeId":"code",
            "_meta":{"sailry":{"expected_revision":changed.revision}}}),
                None,
            )
            .await;
        assert!(accepted.get("result").is_some(), "{accepted}");
        let Output::Session(session) = fixture
            .execute(Command::ReadSession {
                session: fixture.session.id,
            })
            .await
        else {
            panic!("session expected")
        };
        assert_eq!(session.config.mode, WorkMode::Code);
        assert_eq!(session.config.permission, sailry_protocol::Permission::Full);
        assert!(session.revision > changed.revision);
        assert!(fixture.page().await.runs.is_empty());
        editor.close().await;
        fixture.close().await;
    }
}

#[tokio::test]
async fn load_replays_history_and_resume_preserves_it() {
    for remote in [false, true] {
        let fixture = Fixture::new(
            remote,
            models::Server::markdown("Stored result".into()).await,
            false,
        )
        .await;
        let mut editor = fixture.editor().await;
        editor.initialize().await;
        editor.load(&fixture).await;
        assert!(
            editor
                .request(
                    3,
                    "session/prompt",
                    prompt(
                        fixture.session.id,
                        RequestId::new(),
                        fixture.session.revision,
                        "Original turn"
                    ),
                    None
                )
                .await
                .get("result")
                .is_some()
        );
        editor.close().await;
        let mut resumed = fixture.editor().await;
        resumed.initialize().await;
        assert!(
            resumed
                .request(
                    2,
                    "session/resume",
                    json!({"sessionId":fixture.session.id,"cwd":fixture.root(),"mcpServers":[]}),
                    None
                )
                .await
                .get("result")
                .is_some()
        );
        assert!(answer(&resumed.updates).is_empty());
        resumed.close().await;
        let mut loaded = fixture.editor().await;
        loaded.initialize().await;
        assert!(loaded.load(&fixture).await.get("result").is_some());
        assert_eq!(answer(&loaded.updates), "Stored result");
        assert_eq!(fixture.page().await.runs.len(), 1);
        assert_eq!(fixture.model.requests.lock().unwrap().len(), 1);
        loaded.close().await;
        fixture.close().await;
    }
}

#[tokio::test]
async fn restores_complete_history_across_pages() {
    for remote in [false, true] {
        let fixture = Fixture::new(
            remote,
            models::Server::markdown("Paged answer".into()).await,
            false,
        )
        .await;
        let turns = crate::history_fixture::append(&fixture.client, &fixture.session, 25).await;
        let recent = fixture
            .client
            .read_conversation(fixture.session.id, None, 20)
            .await
            .unwrap();
        assert!(!recent.page.runs.iter().any(|run| run.turn == turns[0]));
        let mut editor = fixture.editor().await;
        editor.initialize().await;
        assert!(editor.load(&fixture).await.get("result").is_some());
        assert_eq!(answer(&editor.updates), "Paged answer".repeat(25));
        assert_eq!(fixture.model.requests.lock().unwrap().len(), 25);
        editor.close().await;
        fixture.close().await;
    }
}

#[tokio::test]
async fn leaves_plan_questions_in_node_history() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote, models::Server::start(false).await, false).await;
        let mut editor = fixture.editor().await;
        editor.initialize().await;
        editor.load(&fixture).await;
        let changed = editor
            .request(
                3,
                "session/set_mode",
                json!({"sessionId":fixture.session.id,"modeId":"plan"}),
                None,
            )
            .await;
        let revision = changed["result"]["_meta"]["sailry"]["revision"]
            .as_u64()
            .unwrap();
        let blocked = editor
            .request(
                4,
                "session/prompt",
                prompt(
                    fixture.session.id,
                    RequestId::new(),
                    revision,
                    "Plan the change",
                ),
                None,
            )
            .await;
        assert_eq!(
            blocked["error"]["data"]["sailry"]["state"], "awaiting_input",
            "{blocked}"
        );
        let page = fixture.page().await;
        assert_eq!(page.questions.len(), 1);
        assert_eq!(
            page.questions[0].state,
            sailry_protocol::conversation::question::State::Pending
        );
        assert_eq!(
            json!(page.questions[0].id),
            blocked["error"]["data"]["sailry"]["question"]["id"]
        );
        assert_eq!(page.runs.len(), 1);
        assert_eq!(fixture.model.requests.lock().unwrap().len(), 1);
        editor.close().await;
        fixture.close().await;
    }
}

#[tokio::test]
async fn accepts_resource_links_as_explicit_context() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote, models::Server::start(false).await, false).await;
        let mut editor = fixture.editor().await;
        editor.initialize().await;
        editor.load(&fixture).await;
        let uri = "file:///unregistered/context.txt";
        let completed = editor.request(3, "session/prompt", json!({"sessionId":fixture.session.id,
            "prompt":[{"type":"resource_link","uri":uri,"name":"Context"},{"type":"text","text":"Explain the reference"}]}), None).await;
        assert!(completed.get("result").is_some(), "{completed}");
        let page = fixture.page().await;
        assert!(
            page.entries
                .iter()
                .filter(|entry| entry.author == "user")
                .flat_map(|entry| &entry.parts)
                .any(|part| matches!(part, Part::Text(text) if text.contains(uri)))
        );
        assert!(page.approvals.is_empty());
        assert!(editor.permissions.is_empty());
        assert!(
            editor
                .updates
                .iter()
                .all(|update| update["update"]["sessionUpdate"] != "tool_call")
        );
        editor.close().await;
        fixture.close().await;
    }
}
