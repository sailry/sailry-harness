use super::*;
use sailry_protocol::conversation::question::Question;
use tokio::sync::Notify;

async fn pending(fixture: &Fixture, previous: Option<QuestionId>) -> (Page, Question) {
    let mut stream = fixture
        .client
        .subscribe_conversation(fixture.session.id)
        .await
        .unwrap();
    let mut projection = Projection::new(fixture.client.target(), fixture.session.id, 0);
    tokio::time::timeout(COMPLETION_BUDGET, async {
        loop {
            projection.apply(0, stream.next().await.unwrap()).unwrap();
            let page = &projection.snapshot().unwrap().page;
            if let Some(question) = page.questions.iter().find(|question| {
                question.state == question::State::Pending && Some(question.id) != previous
            }) {
                return (page.clone(), question.clone());
            }
            assert!(
                !page.runs.iter().any(|run| run.status == Status::Failed),
                "{page:?}"
            );
        }
    })
    .await
    .expect("question admission deadline")
}

fn question(prompt: &str) -> (String, serde_json::Value) {
    (
        "ask_user".into(),
        json!({"prompt":prompt,"input":{"kind":"text","multiline":true,"max_bytes":1024}}),
    )
}

async fn answer(fixture: &Fixture, question: &Question, text: &str) {
    fixture
        .client
        .execute(fixture.client.prepare(Command::ResolveQuestion {
            session: fixture.session.id,
            question: question.id,
            response: question::Response::Answer(question::Answer::Text(text.into())),
        }))
        .await
        .unwrap();
}

fn failures(page: &Page) -> Vec<Option<ErrorCode>> {
    page.entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| match part {
            Part::ToolResult { summary, .. } => {
                Some(summary.failure.as_ref().and_then(|failure| failure.code))
            }
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn replay_preserves_the_next_operation() {
    for remote in [false, true] {
        let release = Arc::new(Notify::new());
        let server = Server::held_completion(
            vec![question("First question"), question("Second question")],
            release.clone(),
        )
        .await;
        let mut fixture = Fixture::with_server(remote, server).await;
        tools(&mut fixture).await;
        let active = fixture.push("Ask both questions").await;
        let (_, first) = pending(&fixture, None).await;
        let request = fixture.client.prepare(Command::GuideTurn {
            recipient: active.id,
            expected_revision: active.revision,
            message: "Interrupt the first question and continue".into(),
            immediate: true,
        });
        let admission = fixture.client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let accepted = admission.completion.await.unwrap().unwrap();
        let Output::QueuedTurn(guidance) = &accepted else {
            panic!("guidance turn expected")
        };
        let (page, second) = pending(&fixture, Some(first.id)).await;
        assert_eq!(
            page.questions
                .iter()
                .find(|question| question.id == first.id)
                .unwrap()
                .state,
            question::State::Cancelled
        );
        assert_ne!(first.id, second.id);
        assert_eq!(status(&page, active.id), Status::Running);
        assert_eq!(status(&page, guidance.id), Status::Continued);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
        assert_eq!(fixture.client.execute(request).await.unwrap(), accepted);
        answer(&fixture, &second, "The second operation completed normally").await;
        fixture.server.wait_count(3).await;
        let page = history(&fixture.client, fixture.session.id).await;
        assert_eq!(failures(&page), [Some(ErrorCode::Cancelled), None]);
        assert_eq!(status(&page, active.id), Status::Running);
        let requests = fixture.server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 3);
        let messages = requests[2]["messages"].to_string();
        assert!(messages.contains("The second operation completed normally"));
        assert!(messages.contains("outcome may be unknown"));
        assert!(messages.contains("do not replay"));
        assert_eq!(
            messages
                .matches("Interrupt the first question and continue")
                .count(),
            1
        );
        release.notify_one();
        let page = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(status(&page, active.id), Status::Completed);
        assert_eq!(status(&page, guidance.id), Status::Continued);
        assert!(page.queue.items.is_empty());
        assert!(
            page.entries
                .iter()
                .filter(|entry| entry.author == "assistant")
                .all(|entry| entry.turn == active.id)
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        fixture.close().await;
    }
}

#[derive(Clone, Copy, Debug)]
enum Selection {
    RecipientRevision,
    QueueRevision,
    OtherSession,
    IncompatibleCapture,
}

async fn mismatched(
    fixture: &mut Fixture,
    active: &QueuedTurn,
    selection: Selection,
) -> Option<TurnId> {
    if matches!(selection, Selection::RecipientRevision) {
        let error = fixture
            .client
            .execute(fixture.client.prepare(Command::GuideTurn {
                recipient: active.id,
                expected_revision: active.revision + 1,
                message: "This request must fail".into(),
                immediate: true,
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RevisionConflict);
        return None;
    }
    let mut session = fixture.session.clone();
    if matches!(selection, Selection::OtherSession) {
        let Output::Session(other) = fixture
            .client
            .execute(fixture.client.prepare(Command::CreateSession {
                project: session.project,
                worktree: Some(session.worktree),
                config: Some(session.config.clone()),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        session = other;
    } else if matches!(selection, Selection::IncompatibleCapture) {
        let mut config = session.config.clone();
        config.permission = Permission::Full;
        let Output::Session(changed) = fixture
            .client
            .execute(fixture.client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        session = changed;
    }
    let Output::QueuedTurn(source) = fixture
        .client
        .execute(fixture.client.prepare(Command::QueueTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "This selection must remain independent".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("queued turn expected")
    };
    let result = fixture
        .client
        .execute(fixture.client.prepare(Command::GuideQueuedTurn {
            turn: source.id,
            recipient: active.id,
            expected_revision: if matches!(selection, Selection::QueueRevision) {
                2
            } else {
                1
            },
            immediate: true,
        }))
        .await;
    match selection {
        Selection::QueueRevision => {
            assert_eq!(result.unwrap_err().code, ErrorCode::RevisionConflict)
        }
        Selection::OtherSession => assert_eq!(result.unwrap_err().code, ErrorCode::Conflict),
        Selection::IncompatibleCapture => {
            let Output::Queue(update) = result.unwrap() else {
                panic!("queue update expected")
            };
            let item = update
                .queue
                .items
                .iter()
                .find(|item| item.turn == source.id)
                .unwrap();
            assert_eq!(item.guidance, None);
            assert!(item.ready);
            assert_eq!(source.config.permission, Permission::Full);
            assert_ne!(source.config.permission, active.config.permission);
        }
        Selection::RecipientRevision => unreachable!(),
    }
    Some(source.id)
}

#[tokio::test]
async fn unmatched_admission_keeps_the_tool_running() {
    for remote in [false, true] {
        for selection in [
            Selection::RecipientRevision,
            Selection::QueueRevision,
            Selection::OtherSession,
            Selection::IncompatibleCapture,
        ] {
            let release = Arc::new(Notify::new());
            let server =
                Server::held_completion(vec![question("Keep this operation")], release.clone())
                    .await;
            let mut fixture = Fixture::with_server(remote, server).await;
            tools(&mut fixture).await;
            let active = fixture.push("Wait for the requested answer").await;
            let (_, waiting) = pending(&fixture, None).await;
            let source = mismatched(&mut fixture, &active, selection).await;
            answer(
                &fixture,
                &waiting,
                "The original operation still accepts its answer",
            )
            .await;
            fixture.server.wait_count(2).await;
            let page = history(&fixture.client, fixture.session.id).await;
            assert_eq!(status(&page, active.id), Status::Running, "{selection:?}");
            assert_eq!(failures(&page), [None], "{selection:?}");
            let requests = fixture.server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), 2);
            let messages = requests[1]["messages"].to_string();
            assert!(messages.contains("The original operation still accepts its answer"));
            assert!(!messages.contains("This selection must remain independent"));
            assert!(!messages.contains("This request must fail"));
            if let Some(source) = source {
                let queued = read(&fixture.client, source).await;
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::RemoveQueuedTurn {
                        turn: source,
                        expected_revision: queued.revision,
                    }))
                    .await
                    .unwrap();
            }
            release.notify_one();
            let page = finished(&fixture.client, fixture.session.id, active.id).await;
            assert_eq!(status(&page, active.id), Status::Completed);
            assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
            fixture.close().await;
        }
    }
}
