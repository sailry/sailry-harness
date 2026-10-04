use super::*;

async fn settled(fixture: &Fixture, state: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let value = read(fixture).await;
            let page = history(&fixture.client, fixture.session.id).await;
            if value["state"] == state
                && page.runs.iter().all(|run| {
                    !matches!(
                        run.status,
                        Status::Queued | Status::Running | Status::Stopping
                    )
                })
            {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("goal did not settle")
}

fn submit(fixture: &Fixture, text: &str) -> Request {
    fixture.client.prepare(Command::SubmitTurn {
        session: fixture.session.id,
        expected_revision: 1,
        message: text.to_owned().into(),
    })
}

#[tokio::test]
async fn continues_without_controller_input() {
    for remote in [false, true] {
        let server = Server::goal_loop(3).await;
        let fixture = fixture(remote, &server).await;
        std::fs::write(
            fixture.root.join("evidence.txt"),
            "Verified evidence 中文 🙂",
        )
        .unwrap();
        let request = submit(&fixture, "/goal Verify the evidence in three turns 中文 🙂");
        let first = fixture.client.execute(request.clone()).await.unwrap();
        let value = settled(&fixture, "completed").await;
        assert_eq!(fixture.client.execute(request).await.unwrap(), first);
        assert_eq!(
            value["description"],
            "Verify the evidence in three turns 中文 🙂"
        );
        assert!(value.get("token_budget").is_none());
        let snapshot = observe(&fixture.client, fixture.session.id).await;
        assert_eq!(snapshot.page.runs.len(), 3);
        assert!(
            snapshot
                .page
                .runs
                .iter()
                .all(|run| run.status == Status::Completed)
        );
        assert_eq!(
            results(&snapshot.page)
                .iter()
                .filter(|(name, _)| *name == plugin_tool("files", "read_file"))
                .count(),
            3
        );
        assert_eq!(server.requests.lock().unwrap().len(), 10);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn continues_model_only_work_until_explicit_completion() {
    for remote in [false, true] {
        let server = Server::goal_reasoning(4, "completed").await;
        let fixture = fixture(remote, &server).await;
        fixture
            .client
            .execute(submit(
                &fixture,
                "/goal Develop and verify the written explanation 中文 🙂",
            ))
            .await
            .unwrap();
        let value = settled(&fixture, "completed").await;
        assert_eq!(
            value["description"],
            "Develop and verify the written explanation 中文 🙂"
        );
        assert!(value.get("token_budget").is_none());
        let snapshot = observe(&fixture.client, fixture.session.id).await;
        assert_eq!(snapshot.page.runs.len(), 4);
        assert!(
            snapshot
                .page
                .runs
                .iter()
                .all(|run| run.status == Status::Completed)
        );
        for run in snapshot.page.runs.iter().take(3) {
            let entries: Vec<_> = snapshot
                .page
                .entries
                .iter()
                .filter(|entry| entry.turn == run.turn)
                .collect();
            assert!(
                entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .all(|part| !matches!(part, Part::ToolCall { .. }))
            );
        }
        assert_eq!(results(&snapshot.page).len(), 2);
        assert_eq!(server.requests.lock().unwrap().len(), 6);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn controls_preserve_history_and_ordinary_messages() {
    for remote in [false, true] {
        let server = Server::goal_reasoning(3, "blocked").await;
        let fixture = fixture(remote, &server).await;
        let Output::QueuedTurn(first) = fixture
            .client
            .execute(submit(&fixture, "/goal Verify this task"))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let blocked = settled(&fixture, "blocked").await;
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        let Output::QueuedTurn(normal) = fixture
            .client
            .execute(submit(
                &fixture,
                "An ordinary message after the goal was blocked",
            ))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        assert_eq!(
            finished(&fixture.client, fixture.session.id, normal.id)
                .await
                .runs
                .last()
                .unwrap()
                .status,
            Status::Completed
        );
        assert_eq!(
            server.requests.lock().unwrap().len(),
            5,
            "a blocked goal must not suppress an ordinary model call"
        );
        fixture
            .client
            .execute(submit(&fixture, "/goal resume"))
            .await
            .unwrap();
        let resumed = settled(&fixture, "blocked").await;
        assert_eq!(resumed["id"], blocked["id"]);
        assert_eq!(server.requests.lock().unwrap().len(), 9);
        let Output::PluginResult(inspected) = fixture
            .client
            .execute(submit(&fixture, "/goal"))
            .await
            .unwrap()
        else {
            panic!("plugin result expected")
        };
        assert_eq!(inspected["ui_intent"], "goal");
        assert_eq!(inspected["goal"], resumed);
        let Output::PluginResult(cleared) = fixture
            .client
            .execute(submit(&fixture, "/goal clear"))
            .await
            .unwrap()
        else {
            panic!("plugin result expected")
        };
        assert!(cleared["goal"].is_null());
        assert!(read(&fixture).await.is_null());
        assert!(
            history(&fixture.client, fixture.session.id)
                .await
                .runs
                .iter()
                .any(|run| run.turn == first.id)
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn stops_and_recovers_without_replaying() {
    for remote in [false, true] {
        for restart in [false, true] {
            let server = Server::start(true).await;
            let mut fixture = fixture(remote, &server).await;
            let Output::QueuedTurn(first) = fixture
                .client
                .execute(submit(&fixture, "/goal Verify this task"))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            server.wait_count(1).await;
            if restart {
                let profile = fixture.node.profile().to_owned();
                fixture.node.shutdown().await.unwrap();
                fixture.node = Node::start(profile).await.unwrap();
                fixture.client = Client::new(if remote {
                    fixture
                        .controller
                        .handle()
                        .remote(fixture.node.link().address())
                } else {
                    fixture.node.local()
                });
            } else {
                fixture
                    .client
                    .execute(submit(&fixture, "/goal stop"))
                    .await
                    .unwrap();
            }
            let value = settled(&fixture, "paused").await;
            assert_eq!(value["description"], "Verify this task");
            let page = finished(&fixture.client, fixture.session.id, first.id).await;
            assert!(matches!(
                page.runs.last().unwrap().status,
                Status::Cancelled | Status::Interrupted
            ));
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            let Output::QueuedTurn(ordinary) = fixture
                .client
                .execute(submit(&fixture, "An ordinary message after interruption"))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            server.wait_count(2).await;
            fixture
                .client
                .execute(submit(&fixture, "/goal clear"))
                .await
                .unwrap();
            assert_eq!(
                history(&fixture.client, fixture.session.id)
                    .await
                    .runs
                    .last()
                    .unwrap()
                    .status,
                Status::Running,
                "clearing a stopped goal must not cancel unrelated input"
            );
            fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(Command::StopTurn { turn: ordinary.id }),
                )
                .await
                .unwrap();
            finished(&fixture.client, fixture.session.id, ordinary.id).await;
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn disable_and_remove_stop_owned_automatic_work() {
    for remote in [false, true] {
        for remove in [false, true] {
            let start = Arc::new(tokio::sync::Notify::new());
            let finish = Arc::new(tokio::sync::Notify::new());
            let server = Server::held(start.clone(), finish.clone()).await;
            let fixture = fixture(remote, &server).await;
            fixture
                .client
                .execute(submit(&fixture, "/goal Verify this task"))
                .await
                .unwrap();
            server.wait_count(1).await;
            start.notify_one();
            finish.notify_one();
            server.wait_count(2).await;
            let automatic = history(&fixture.client, fixture.session.id)
                .await
                .runs
                .last()
                .unwrap()
                .turn;
            let command = if remove {
                Command::RemovePlugin {
                    name: "goals".into(),
                    expected_revision: 3,
                }
            } else {
                Command::SetPluginEnabled {
                    name: "goals".into(),
                    expected_revision: 3,
                    enabled: false,
                }
            };
            fixture
                .client
                .execute(fixture.client.prepare(command))
                .await
                .unwrap();
            let page = finished(&fixture.client, fixture.session.id, automatic).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Cancelled);
            assert_eq!(page.runs.len(), 2);
            assert_eq!(server.requests.lock().unwrap().len(), 2);
            if !remove {
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::SetPluginEnabled {
                        name: "goals".into(),
                        expected_revision: 4,
                        enabled: true,
                    }))
                    .await
                    .unwrap();
                assert_eq!(read(&fixture).await["state"], "paused");
            }
            let profile = fixture.node.profile().to_owned();
            fixture.node.shutdown().await.unwrap();
            let node = Node::start(profile).await.unwrap();
            let client = Client::new(if remote {
                fixture.controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            assert_eq!(history(&client, fixture.session.id).await, page);
            if remove {
                let Output::Plugins(packages) = client
                    .execute(client.prepare(Command::ListPlugins))
                    .await
                    .unwrap()
                else {
                    panic!("packages expected")
                };
                assert!(!packages.iter().any(|package| package.name == "goals"));
            }
            assert_eq!(server.requests.lock().unwrap().len(), 2);
            node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
