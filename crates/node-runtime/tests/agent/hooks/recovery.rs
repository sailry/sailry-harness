use super::*;

#[tokio::test]
async fn never_replays_end_after_restart() {
    for remote in [false, true] {
        let server = Server::start(false).await;
        let mut fixture = Fixture::packages(remote, &server, &[]).await;
        ready(&mut fixture, WorkMode::Code).await;
        save(
            &fixture.client,
            fixture.session.worktree,
            Settings {
                enabled: true,
                turn_end: script("printf '%s\\n' \"$SAILRY_HOOK_TURN\" >> ends.txt; /bin/sleep 2; printf late > late.txt"),
                additional: vec![entry("end-later", Phase::TurnEnd, "printf later > later-script.txt", true)],
                ..Default::default()
            },
        ).await;

        let request = fixture.client.prepare(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Finish the fixture turn".into(),
        });
        let Output::QueuedTurn(turn) = fixture.client.execute(request.clone()).await.unwrap()
        else {
            panic!("turn expected")
        };
        let expected = format!("{}\n", turn.id);
        tokio::time::timeout(Duration::from_secs(5), async {
            while !std::fs::read_to_string(fixture.root.join("ends.txt"))
                .is_ok_and(|content| content == expected)
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("hook output deadline");
        let Fixture {
            directory,
            root,
            node,
            controller,
            client,
            session,
        } = fixture;
        let identity = node.id();
        node.shutdown().await.unwrap();
        drop(client);
        controller.handle().disconnect(identity).await;
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let page = history(&client, session.id).await;
        let recovered = page.runs.iter().find(|run| run.turn == turn.id).unwrap();
        assert_eq!(recovered.status, Status::Interrupted, "{recovered:?}");
        assert_eq!(
            recovered.error.as_ref().unwrap().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::QueuedTurn(turn.clone())
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            std::fs::read_to_string(root.join("ends.txt")).unwrap(),
            format!("{}\n", turn.id)
        );
        assert!(!root.join("late.txt").exists());
        assert!(!root.join("later-script.txt").exists());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        let db =
            rusqlite::Connection::open(directory.path().join("node/storage/node.sqlite3")).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM hook_executions WHERE turn=?1 AND phase='turn_end'",
                [turn.id.to_string()],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }
}

#[tokio::test]
async fn confines_scripts_without_granting_tool_access() {
    for remote in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let curl = which::which("curl").expect("hook sandbox acceptance requires curl");
        let read_tool = plugin_tool("files", "read_file");
        let write_tool = plugin_tool("files", "write_file");
        let server = Server::tools(vec![
            (read_tool.clone(), json!({"path":"source.txt"})),
            (
                write_tool.clone(),
                json!({"path":"denied.txt","text":"must remain denied","expected_revision":null}),
            ),
        ])
        .await;
        let fixture = Fixture::packages(remote, &server, &["files"]).await;
        std::fs::write(fixture.root.join("source.txt"), "hook fixture").unwrap();
        let command = format!(
            "if printf outside > ../outside.txt; then exit 91; fi; if '{}' --silent --max-time 1 http://{address}/; then exit 92; fi; printf confined > allowed.txt; printf '%s\\n' \"$SAILRY_HOOK_TOOL\" >> called.txt",
            curl.to_str().unwrap().replace('\'', "'\\''")
        );
        save(
            &fixture.client,
            fixture.session.worktree,
            Settings {
                enabled: true,
                before_tool: script(&command),
                ..Default::default()
            },
        )
        .await;
        let turn = approvals::rules::submit(&fixture.client, &fixture.session).await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        assert_eq!(approval.source, ApprovalSource::User);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("allowed.txt")).unwrap(),
            "confined"
        );
        assert!(!fixture.directory.path().join("outside.txt").exists());
        assert!(!fixture.root.join("denied.txt").exists());
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
        // ADK runs attempt callbacks before confirmation; they do not authorize the tool.
        let attempted = format!("{read_tool}\n{write_tool}\n");
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("called.txt")).unwrap(),
            attempted
        );
        process::decide(&fixture.client, &approval, Decision::Deny).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.approvals
                .iter()
                .find(|value| value.id == approval.id)
                .unwrap()
                .state,
            ApprovalState::Denied
        );
        assert!(!fixture.root.join("denied.txt").exists());
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("called.txt")).unwrap(),
            attempted
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn records_end_once_for_each_turn() {
    for remote in [false, true] {
        let server = Server::start(false).await;
        let mut fixture = Fixture::packages(remote, &server, &[]).await;
        ready(&mut fixture, WorkMode::Code).await;
        save(
            &fixture.client,
            fixture.session.worktree,
            Settings {
                enabled: true,
                turn_end: script("printf '%s\\n' \"$SAILRY_HOOK_TURN\" >> ends.txt"),
                ..Default::default()
            },
        )
        .await;
        let mut turns = Vec::new();
        for _ in 0..2 {
            let request = fixture.client.prepare(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Finish the fixture turn".into(),
            });
            let Output::QueuedTurn(turn) = fixture.client.execute(request.clone()).await.unwrap()
            else {
                panic!("turn expected")
            };
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(
                page.runs
                    .iter()
                    .find(|run| run.turn == turn.id)
                    .unwrap()
                    .status,
                Status::Completed
            );
            assert_eq!(
                fixture.client.execute(request).await.unwrap(),
                Output::QueuedTurn(turn.clone())
            );
            turns.push(turn.id);
        }
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("ends.txt")).unwrap(),
            turns
                .iter()
                .map(|turn| format!("{turn}\n"))
                .collect::<String>()
        );
        let directory = fixture.directory.path().to_path_buf();
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
        let db = rusqlite::Connection::open(directory.join("node/storage/node.sqlite3")).unwrap();
        let mut query = db
            .prepare("SELECT body FROM agent_events WHERE session=?1")
            .unwrap();
        let events = query
            .query_map([fixture.session.id.to_string()], |row| {
                row.get::<_, Vec<u8>>(0)
            })
            .unwrap()
            .map(|row| serde_json::from_slice::<adk_core::Event>(&row.unwrap()).unwrap())
            .collect::<Vec<_>>();
        let hooks = events
            .iter()
            .filter_map(|event| event.provider_metadata.get("sailry.hook"))
            .map(|body| serde_json::from_str::<serde_json::Value>(body).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(hooks.len(), 2);
        assert!(
            hooks
                .iter()
                .all(|record| record["phase"] == "turn_end" && record["revision"] == 1)
        );
    }
}
