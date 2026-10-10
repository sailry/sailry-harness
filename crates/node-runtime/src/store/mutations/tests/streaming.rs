use super::*;
use crate::store::{Job, agent::Operation};
use adk_core::{Content, Event};
use sailry_protocol::conversation::{Change, Part, Status};
use tokio::sync::oneshot;

fn event(id: &str, text: &str, partial: bool) -> Event {
    let mut event = Event::new("stream-fixture");
    event.id = id.into();
    event.author = "assistant".into();
    event.set_content(Content::new("model").with_text(text));
    event.llm_response.partial = partial;
    event
}

async fn session(client: &Client) -> Session {
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    let Output::Session(session) = client
        .execute(client.prepare(Command::CreateSession {
            project: Some(snapshot.projects[0].id),
            worktree: None,
            config: Some(SessionConfig {
                assistant: None,
                resource: None,
                provider: ProviderId::new(),
                model: "fixture".into(),
                effort: Effort::Low,
                mode: WorkMode::Code,
                permission: Permission::Ask,
                credential: None,
            }),
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    session
}

async fn append(
    fixture: &Fixture,
    session: SessionId,
    turn: TurnId,
    event: Event,
) -> oneshot::Receiver<adk_core::Result<()>> {
    let (reply, response) = oneshot::channel();
    fixture
        .store
        .ingress
        .sender
        .send(Job::Agent(Box::new(Operation::Append {
            session,
            turn,
            event,
            reply,
        })))
        .await
        .unwrap();
    response
}

#[tokio::test]
async fn bypasses_blocked_storage_and_file_workers() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    for remote in [false, true] {
        let (fixture, mut gate) = blocked().await;
        let network = Link::bind(
            &fixture.identity,
            NetworkScope::default(),
            fixture.store.ingress.clone(),
            fixture.store.ingress.clone(),
        )
        .await
        .unwrap();
        let controller = Link::controller(
            fixture.directory.path().join("stream-controller"),
            NetworkScope::default(),
        )
        .await
        .unwrap();
        let address = controller
            .handle()
            .pair(network.handle().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = if remote {
            Client::new(controller.handle().remote(address))
        } else {
            Client::new(Arc::new(Local::new(
                fixture.identity.id(),
                fixture.identity.id(),
                fixture.store.ingress.clone(),
            )))
        };
        let session = session(&client).await;
        let foreign = self::session(&client).await;
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::QueueTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Stream fixture".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("queued turn expected")
        };
        client
            .execute(client.prepare(Command::StartQueuedTurn { turn: turn.id }))
            .await
            .unwrap();
        let token = sailry_link::CancellationToken::new();
        let invocation = fixture
            .store
            .ingress
            .claim_run(token.clone())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(invocation.turn.id, turn.id);
        let mut stream = client.subscribe_conversation(session.id).await.unwrap();
        assert!(matches!(
            stream.next().await.unwrap(),
            Update::ConversationSnapshot(_)
        ));
        let mut other = client.subscribe_conversation(foreign.id).await.unwrap();
        assert!(matches!(
            other.next().await.unwrap(),
            Update::ConversationSnapshot(_)
        ));
        let file = client
            .dispatch(fixture.write("blocked", "saved file"))
            .await
            .unwrap();
        gate.entered.recv().await.unwrap();

        // Hold the actual storage actor, then queue a canonical append behind it.
        let (entered, waiting) = mpsc::unbounded_channel();
        let (release, released) = std::sync::mpsc::channel();
        let mut storage_gate = Gate {
            entered: waiting,
            release,
        };
        fixture
            .store
            .ingress
            .sender
            .send(Job::Barrier {
                entered,
                release: released,
            })
            .await
            .unwrap();
        storage_gate.entered.recv().await.unwrap();
        let mut pending = append(
            &fixture,
            session.id,
            turn.id,
            event("prefix", "Persisted prefix", false),
        )
        .await;
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut pending)
                .await
                .is_err()
        );
        let mut sequence = 0;
        for text in ["first ", "second"] {
            tokio::time::timeout(
                Duration::from_millis(500),
                fixture
                    .store
                    .ingress
                    .partial(session.id, turn.id, event("answer", text, true)),
            )
            .await
            .expect("partial publication must bypass the blocked storage worker")
            .unwrap();
            let Update::ConversationFrame(frame) =
                tokio::time::timeout(Duration::from_millis(500), stream.next())
                    .await
                    .expect("subscribers must receive output while storage is blocked")
                    .unwrap()
            else {
                panic!("frame expected")
            };
            assert!(frame.sequence > sequence);
            sequence = frame.sequence;
            assert_eq!(frame.session, session.id);
            assert!(
                matches!(frame.change, Change::Delta(draft) if draft.turn == turn.id && draft.parts == vec![Part::Text(text.into())])
            );
        }
        assert_eq!(
            fixture
                .store
                .ingress
                .partial(
                    foreign.id,
                    turn.id,
                    event("private", "private answer", true),
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(20), other.next())
                .await
                .is_err()
        );
        storage_gate.release.send(()).unwrap();
        pending.await.unwrap().unwrap();
        append(
            &fixture,
            session.id,
            turn.id,
            event("answer", "first second", false),
        )
        .await
        .await
        .unwrap()
        .unwrap();
        // Late output cannot recreate the draft after its canonical entry is published.
        fixture
            .store
            .ingress
            .partial(session.id, turn.id, event("answer", "late", true))
            .await
            .unwrap();
        let mut committed = client.subscribe_conversation(session.id).await.unwrap();
        let Update::ConversationSnapshot(committed) = committed.next().await.unwrap() else {
            panic!("snapshot expected")
        };
        assert!(committed.drafts.is_empty());
        fixture
            .store
            .ingress
            .finish_run(turn.id, Status::Completed, None, None)
            .await
            .unwrap();
        fixture
            .store
            .ingress
            .partial(session.id, turn.id, event("after", "late terminal", true))
            .await
            .unwrap();
        fixture.store.ingress.agents.finish(turn.id);
        let mut reopened = client.subscribe_conversation(session.id).await.unwrap();
        let Update::ConversationSnapshot(snapshot) = reopened.next().await.unwrap() else {
            panic!("snapshot expected")
        };
        assert!(snapshot.drafts.is_empty());
        assert!(snapshot.sequence > sequence);
        assert!(
            snapshot
                .page
                .runs
                .iter()
                .any(|run| run.turn == turn.id && run.status == Status::Completed)
        );
        assert_eq!(
            snapshot
                .page
                .entries
                .iter()
                .map(|entry| (entry.id.clone(), entry.parts.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("prefix".into(), vec![Part::Text("Persisted prefix".into())]),
                ("answer".into(), vec![Part::Text("first second".into())])
            ]
        );
        let database =
            rusqlite::Connection::open(fixture.directory.path().join("node.sqlite3")).unwrap();
        let persisted: Vec<Event> = database
            .prepare("SELECT body FROM agent_events WHERE turn=?1 ORDER BY sequence")
            .unwrap()
            .query_map([turn.id.to_string()], |row| row.get::<_, Vec<u8>>(0))
            .unwrap()
            .map(|body| serde_json::from_slice(&body.unwrap()).unwrap())
            .collect();
        assert_eq!(
            persisted
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            vec!["prefix", "answer"]
        );
        assert!(persisted.iter().all(|event| !event.llm_response.partial));
        assert_eq!(
            persisted[1].content().unwrap().parts,
            vec![adk_core::Part::Text {
                text: "first second".into()
            }]
        );
        let Output::QueuedTurn(cancelled) = client
            .execute(client.prepare(Command::QueueTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Cancelled stream fixture".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("queued turn expected")
        };
        client
            .execute(client.prepare(Command::StartQueuedTurn { turn: cancelled.id }))
            .await
            .unwrap();
        fixture
            .store
            .ingress
            .claim_run(sailry_link::CancellationToken::new())
            .await
            .unwrap()
            .unwrap();
        fixture
            .store
            .ingress
            .partial(
                session.id,
                cancelled.id,
                event("cancelled", "before stop", true),
            )
            .await
            .unwrap();
        client
            .execute(client.prepare(Command::StopTurn { turn: cancelled.id }))
            .await
            .unwrap();
        fixture
            .store
            .ingress
            .partial(
                session.id,
                cancelled.id,
                event("late-stop", "after stop", true),
            )
            .await
            .unwrap();
        let mut stopping = client.subscribe_conversation(session.id).await.unwrap();
        let Update::ConversationSnapshot(stopping) = stopping.next().await.unwrap() else {
            panic!("snapshot expected")
        };
        assert_eq!(stopping.drafts.len(), 1);
        assert_eq!(
            stopping.drafts[0].parts,
            vec![Part::Text("before stop".into())]
        );
        assert_eq!(
            fixture
                .store
                .ingress
                .finish_run(cancelled.id, Status::Completed, None, None)
                .await
                .unwrap(),
            Status::Cancelled
        );
        fixture.store.ingress.agents.finish(cancelled.id);
        gate.release.send(()).unwrap();
        file.completion.await.unwrap().unwrap();
        client
            .execute(client.prepare(Command::RemoveSession {
                session: session.id,
                expected_revision: session.revision,
            }))
            .await
            .unwrap();
        fixture
            .store
            .ingress
            .partial(session.id, turn.id, event("removed", "hidden", true))
            .await
            .unwrap();
        let rejection = match client.subscribe_conversation(session.id).await {
            Err(fault) => fault,
            // Paired subscriptions receive resource rejection in their first frame.
            Ok(mut stream) => stream.next().await.unwrap_err(),
        };
        assert_eq!(rejection.code, ErrorCode::NotFound);
        controller.close().await.unwrap();
        fixture.store.stop_admission();
        network.close().await.unwrap();
        fixture.store.shutdown().await.unwrap();
    }
}
