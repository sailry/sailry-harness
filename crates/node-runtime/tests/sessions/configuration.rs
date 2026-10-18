use super::*;

#[tokio::test]
async fn freezes_delegation_policy_across_resume() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = Node::start_empty(&profile).await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let config = SessionConfig {
            assistant: None,
            resource: None,
            provider: ProviderId::new(),
            model: "fixture".into(),
            effort: Effort::Default,
            mode: WorkMode::Code,
            permission: Permission::Ask,
            delegation_proactive: true,
            credential: None,
        };
        client
            .execute(client.prepare(Command::SetDefaults {
                expected_revision: 0,
                config: config.clone(),
            }))
            .await
            .unwrap();
        let Output::Session(session) = client
            .execute(client.prepare(Command::CreateSession {
                project: None,
                worktree: None,
                config: None,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected");
        };
        assert_eq!(session.config, config);
        let request = client.prepare(Command::QueueTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Capture the coordinator policy".into(),
        });
        let Output::QueuedTurn(queued) = client.execute(request.clone()).await.unwrap() else {
            panic!("queued turn expected");
        };
        let mut standard = session.config.clone();
        standard.delegation_proactive = false;
        client
            .execute(client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config: standard.clone(),
            }))
            .await
            .unwrap();
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::QueuedTurn(queued.clone())
        );
        assert!(queued.config.delegation_proactive);
        let state = snapshot(&client).await;
        assert_eq!(state.sessions[0].config, standard);
        assert_eq!(
            client
                .read_turn_capture(session.id, queued.id)
                .await
                .unwrap()
                .config,
            config
        );
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start_empty(&profile).await.unwrap();
        let resumed = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let state = snapshot(&resumed).await;
        assert_eq!(state.sessions[0].config, standard);
        assert_eq!(
            resumed
                .read_turn_capture(session.id, queued.id)
                .await
                .unwrap()
                .config,
            config
        );
        assert_eq!(state.turns[0].id, queued.id);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn defers_unused_archive_validation() {
    for remote in [false, true] {
        for owner in ["receipt", "capture", "history"] {
            let directory = tempfile::tempdir().unwrap();
            let profile = directory.path().join("node");
            let node = Node::start_empty(&profile).await.unwrap();
            let controller =
                Link::controller(directory.path().join("controller"), NetworkScope::default())
                    .await
                    .unwrap();
            controller
                .handle()
                .pair(node.link().invite().unwrap().ticket())
                .await
                .unwrap();
            let client = Client::new(if remote {
                controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            let config = SessionConfig {
                assistant: None,
                resource: None,
                provider: ProviderId::new(),
                model: "fixture".into(),
                effort: Effort::Default,
                mode: WorkMode::Code,
                permission: Permission::Ask,
                delegation_proactive: false,
                credential: None,
            };
            let Output::Session(session) = client
                .execute(client.prepare(Command::CreateSession {
                    project: None,
                    worktree: None,
                    config: Some(config.clone()),
                }))
                .await
                .unwrap()
            else {
                panic!("session expected")
            };
            let request = client.prepare(Command::QueueTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Archived input".into(),
            });
            let Output::QueuedTurn(turn) = client.execute(request.clone()).await.unwrap() else {
                panic!("turn expected")
            };
            assert_eq!(
                client.outcome(&request).await.unwrap(),
                RequestOutcome::Completed(Box::new(Ok(Output::QueuedTurn(turn.clone())))),
                "original receipt must belong to the reading caller (remote={remote})"
            );
            client
                .execute(client.prepare(Command::RemoveQueuedTurn {
                    turn: turn.id,
                    expected_revision: 1,
                }))
                .await
                .unwrap();
            let mut changed = config;
            changed.model = "current".into();
            client
                .execute(client.prepare(Command::SetSessionConfig {
                    session: session.id,
                    expected_revision: session.revision,
                    config: changed.clone(),
                }))
                .await
                .unwrap();
            drop(client);
            node.shutdown().await.unwrap();
            let path = profile.join("storage/node.sqlite3");
            let invalid = "{\"private_marker\":true}";
            let corrupt = match owner {
                "receipt" => serde_json::from_str::<Result<Output, Fault>>(invalid).is_err(),
                "capture" => serde_json::from_str::<SessionConfig>(invalid).is_err(),
                "history" => serde_json::from_str::<adk_core::Event>(invalid).is_err(),
                _ => unreachable!(),
            };
            assert!(corrupt, "{owner} fixture must be invalid (remote={remote})");
            let db = rusqlite::Connection::open(&path).unwrap();
            if owner == "capture" {
                let updated = db
                    .execute(
                        "UPDATE session_revisions SET config=?1 WHERE session=?2 AND revision=?3",
                        rusqlite::params![
                            invalid.as_bytes(),
                            session.id.to_string(),
                            session.revision as i64
                        ],
                    )
                    .unwrap();
                assert_eq!(updated, 1, "capture injection (remote={remote})");
            }
            if owner == "receipt" {
                let updated = db
                    .execute(
                        "UPDATE requests SET result=?1 WHERE id=?2",
                        rusqlite::params![invalid, request.id.to_string()],
                    )
                    .unwrap();
                assert_eq!(updated, 1, "receipt injection (remote={remote})");
            }
            if owner == "history" {
                db.execute(
            "INSERT INTO agent_events(session,id,turn,body) VALUES(?1,'invalid-archive',?2,?3)",
            rusqlite::params![
                session.id.to_string(),
                turn.id.to_string(),
                invalid.as_bytes()
            ],
        )
        .unwrap();
                db.execute(
                    "INSERT INTO conversation_events(session,sequence) VALUES(?1,?2)",
                    rusqlite::params![session.id.to_string(), db.last_insert_rowid()],
                )
                .unwrap();
            }
            db.close().unwrap();
            let node = Node::start_empty(&profile).await.unwrap();
            let client = Client::new(if remote {
                controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            assert_eq!(snapshot(&client).await.sessions[0].config, changed);
            let error = match owner {
                "receipt" => client.outcome(&request).await.err(),
                "capture" => client.read_turn_capture(session.id, turn.id).await.err(),
                "history" => client.read_conversation(session.id, None, 20).await.err(),
                _ => unreachable!(),
            }
            .unwrap_or_else(|| panic!("{owner} archive must fail when read (remote={remote})"));
            assert_eq!(error.code, ErrorCode::Internal);
            assert!(!error.message.contains("private_marker"));
            node.shutdown().await.unwrap();
            let db = rusqlite::Connection::open(&path).unwrap();
            let sql = match owner {
                "receipt" => "SELECT CAST(result AS BLOB) FROM requests WHERE id=?1",
                "capture" => "SELECT config FROM session_revisions WHERE session=?1 AND revision=1",
                "history" => "SELECT body FROM agent_events WHERE id=?1",
                _ => unreachable!(),
            };
            let id = match owner {
                "receipt" => request.id.to_string(),
                "capture" => session.id.to_string(),
                "history" => "invalid-archive".into(),
                _ => unreachable!(),
            };
            let stored: Vec<u8> = db.query_row(sql, [id], |row| row.get(0)).unwrap();
            assert_eq!(stored, invalid.as_bytes());
            db.execute_batch("DROP INDEX events_kind; PRAGMA journal_mode=DELETE")
                .unwrap();
            db.close().unwrap();
            let before = std::fs::read(&path).unwrap();
            let error = Node::start_empty(&profile)
                .await
                .err()
                .expect("upgrade must reject invalid archives");
            assert!(!error.to_string().contains("private_marker"));
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert!(!profile.join("storage/backups").exists());
            controller.close().await.unwrap();
        }
    }
}
