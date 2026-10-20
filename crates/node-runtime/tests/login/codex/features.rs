use super::*;
use std::{collections::BTreeMap, sync::Mutex};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn freezes_choices_and_sends_priority() {
    for remote in [false, true] {
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let received = bodies.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if request.path.starts_with("/models") {
                let mut models = server::models();
                models["models"][0]["service_tiers"] = json!([{"id":"priority","name":"Priority"}]);
                models["models"][0]["default_service_tier"] = json!("priority");
                return Reply::Json(models);
            }
            if request.path == "/responses" {
                let body = server::input(
                    request,
                    &Options::defaults(Authentication::ChatGptOAuth).unwrap(),
                    ACCESS,
                );
                received.lock().unwrap().push(body);
                return server::stream(
                    vec![
                        server::delta("Completed"),
                        server::completed(json!([server::message("Completed")])),
                    ],
                    true,
                );
            }
            server::standard(request, 3600)
        })
        .await;
        let fixture = Fixture::packages(remote, &server, &["files"]).await;
        let provider = configured(&fixture, None).await;
        assert_eq!(provider.models[0].context, 8192);
        let advertised = &provider.models[0].capabilities.as_ref().unwrap().features;
        assert_eq!(advertised[0].id, "speed");
        assert_eq!(advertised[0].options[0].id, "fast");
        let mut session = super::super::execution::session(&fixture, &provider).await;
        assert!(session.config.features.is_empty());
        let effort = session.config.effort;
        let mut selected = session.config.clone();
        selected.features = BTreeMap::from([("speed".into(), "fast".into())]);
        let Output::Session(updated) = fixture
            .client
            .execute(fixture.client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config: selected.clone(),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        session = updated;
        let request = fixture.client.prepare(Command::QueueTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Capture processing choice".into(),
        });
        let queued = fixture.client.execute(request.clone()).await.unwrap();
        let Output::QueuedTurn(turn) = &queued else {
            panic!("queued turn expected")
        };
        assert_eq!(turn.config.features, selected.features);
        assert_eq!(turn.config.effort, effort);
        let revision = session.revision;
        let mut ordinary = selected.clone();
        ordinary.features.clear();
        let Output::Session(updated) = fixture
            .client
            .execute(fixture.client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: revision,
                config: ordinary.clone(),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        session = updated;
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::SetSessionConfig {
                    session: session.id,
                    expected_revision: revision,
                    config: selected.clone(),
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        for (model, value) in [("fixture", "ultrafast"), ("unknown", "fast")] {
            let mut invalid = selected.clone();
            invalid.model = model.into();
            invalid.features.insert("speed".into(), value.into());
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::SetSessionConfig {
                        session: session.id,
                        expected_revision: session.revision,
                        config: invalid,
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        let fixture = super::super::execution::renewals::restart(fixture, &server, remote).await;
        assert_eq!(fixture.client.execute(request).await.unwrap(), queued);
        let Output::Snapshot(snapshot) = fixture
            .client
            .execute(fixture.client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        session = snapshot
            .sessions
            .into_iter()
            .find(|value| value.id == session.id)
            .unwrap();
        assert_eq!(session.config, ordinary);
        fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::StartQueuedTurn { turn: turn.id }),
            )
            .await
            .unwrap();
        let queue = fixture
            .client
            .read_conversation(session.id, None, 10)
            .await
            .unwrap()
            .page
            .queue;
        if queue.paused {
            fixture
                .client
                .execute(fixture.client.prepare(Command::SetQueuePaused {
                    session: session.id,
                    expected_revision: queue.revision,
                    paused: false,
                }))
                .await
                .unwrap();
        }
        super::super::execution::finished(&fixture.client, turn, Status::Completed).await;
        let (_, ordinary_turn) = super::super::execution::submit(&fixture.client, &session).await;
        super::super::execution::finished(&fixture.client, &ordinary_turn, Status::Completed).await;
        assert!(ordinary_turn.config.features.is_empty());
        {
            let bodies = bodies.lock().unwrap();
            assert_eq!(bodies.len(), 2);
            assert_eq!(bodies[0]["service_tier"], "priority");
            assert!(bodies[1].get("service_tier").is_none());
            assert_eq!(bodies[0]["reasoning"], bodies[1]["reasoning"]);
            assert_eq!(bodies[0]["model"], bodies[1]["model"]);
            assert_eq!(bodies[0]["store"], bodies[1]["store"]);
        }
        fixture.close().await;
    }
}
