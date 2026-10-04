use super::*;
use sailry_client::Projection;

pub(super) async fn session(fixture: &process::Fixture) -> Session {
    let Output::Session(session) = execute(
        &fixture.client,
        Command::ReadSession {
            session: fixture.session.id,
        },
    )
    .await
    else {
        panic!("session expected")
    };
    session
}

#[tokio::test]
async fn refreshes_next_input() {
    for remote in [false, true] {
        let model = Server::turn_tools(vec![(alias("native", "read"), json!({}))]).await;
        let mut fixture = process::Fixture::new(remote, &model).await;
        isolate(&fixture.client).await;
        let original = fixture.session.clone();
        let (_, empty) = submit(&fixture, true).await;
        assert!(empty.plugins.is_empty());

        let mut updates = fixture.client.subscribe().await.unwrap();
        let mut projection = Projection::new(fixture.node.id(), 1);
        projection.apply(1, updates.next().await.unwrap()).unwrap();
        let config = json!({"native": peer::config("mcp::peer::stdio_peer", "normal")});
        package(&fixture.root, config.clone(), "first");
        let first = install(&fixture, 0).await;
        assert_eq!(session(&fixture).await.revision, original.revision);

        // Failed input must not change the session configuration.
        let mut message: Input = "Invalid reference".into();
        message.references.push(reference::Reference {
            target: reference::Target::File("../outside".into()),
            label: "Outside".into(),
        });
        assert!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::QueueTurn {
                    session: original.id,
                    expected_revision: original.revision,
                    message,
                }))
                .await
                .is_err()
        );
        assert_eq!(session(&fixture).await.revision, original.revision);

        let (request, queued) = submit(&fixture, true).await;
        assert_eq!(queued.plugins, [first.summary.reference()]);
        assert_eq!(queued.revision, original.revision);
        fixture.session = session(&fixture).await;
        assert_eq!(fixture.session.config, original.config);
        assert_eq!(fixture.session.roles, original.roles);
        assert_eq!(fixture.session.profile, original.profile);

        tokio::time::timeout(Duration::from_secs(5), async {
            while !projection
                .snapshot()
                .unwrap()
                .turns
                .iter()
                .any(|turn| turn.id == queued.id)
            {
                projection.apply(1, updates.next().await.unwrap()).unwrap();
            }
        })
        .await
        .expect("admission update deadline");
        assert_eq!(projection.snapshot().unwrap().sessions[0], fixture.session);
        assert_eq!(
            fixture.client.execute(request).await.unwrap(),
            Output::QueuedTurn(queued.clone())
        );
        let fault = fixture
            .client
            .execute(fixture.client.prepare(Command::QueueTurn {
                session: original.id,
                expected_revision: original.revision - 1,
                message: "Stale configuration".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(fault.code, ErrorCode::RevisionConflict);
        let (_, same) = submit(&fixture, true).await;
        assert_eq!(same.revision, queued.revision);

        package(&fixture.root, config, "second");
        let second = install(&fixture, first.summary.revision).await;
        let (_, latest) = submit(&fixture, true).await;
        assert_eq!(latest.plugins, [second.summary.reference()]);
        assert_eq!(latest.revision, queued.revision);
        fixture.session = session(&fixture).await;
        execute(
            &fixture.client,
            Command::SetPluginEnabled {
                name: "example".into(),
                expected_revision: second.summary.revision,
                enabled: false,
            },
        )
        .await;
        let (_, disabled) = submit(&fixture, true).await;
        assert!(disabled.plugins.is_empty());
        assert_eq!(disabled.revision, latest.revision);
        fixture.session = session(&fixture).await;
        execute(
            &fixture.client,
            Command::SetPluginEnabled {
                name: "example".into(),
                expected_revision: second.summary.revision + 1,
                enabled: true,
            },
        )
        .await;
        let (_, enabled) = submit(&fixture, true).await;
        assert_eq!(enabled.plugins, latest.plugins);
        fixture.session = session(&fixture).await;
        execute(
            &fixture.client,
            Command::RemovePlugin {
                name: "example".into(),
                expected_revision: second.summary.revision + 2,
            },
        )
        .await;
        let (_, removed) = submit(&fixture, true).await;
        assert!(removed.plugins.is_empty());

        for frozen in [&empty, &queued, &latest] {
            let Output::QueuedMessage(restored) =
                execute(&fixture.client, Command::ReadQueuedTurn { turn: frozen.id }).await
            else {
                panic!("queued message expected")
            };
            assert_eq!(&restored.turn, frozen);
        }
        // Frozen packages remain executable after update and removal.
        for (turn, version) in [(&queued, "first"), (&latest, "second")] {
            execute(&fixture.client, Command::StartQueuedTurn { turn: turn.id }).await;
            let page = finished(&fixture.client, original.id, turn.id).await;
            assert_eq!(
                page.runs
                    .iter()
                    .find(|run| run.turn == turn.id)
                    .unwrap()
                    .status,
                Status::Completed
            );
            assert_eq!(results(&page).last().unwrap()["output"]["version"], version);
        }
        {
            let requests = model.requests.lock().unwrap();
            assert!(requests.iter().any(|request| {
                request["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|tool| tool["function"]["name"] == alias("native", "read"))
            }));
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
