use super::*;
use sailry_link::CancellationToken;

async fn guide_parent(fixture: &Fixture, active: &QueuedTurn, text: &str) -> QueuedTurn {
    let admission = fixture
        .client
        .dispatch(fixture.client.prepare(Command::GuideTurn {
            recipient: active.id,
            expected_revision: active.revision,
            message: text.into(),
        }))
        .await
        .unwrap();
    assert!(admission.receipt.durable);
    let Output::QueuedTurn(turn) = admission.completion.await.unwrap().unwrap() else {
        panic!("guidance turn expected")
    };
    turn
}

fn user_text(request: &Value) -> String {
    crate::agent_support::task_request(request)["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "user")
        .map(|message| message_text(&message["content"]))
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_live(page: &Page, parent: TurnId, child: &Run) {
    assert_eq!(status(page, parent), Status::Running);
    let observed = page
        .children
        .iter()
        .find(|item| item.run.turn == child.turn)
        .unwrap();
    assert_eq!(observed.run.session, child.session);
    assert_eq!(observed.run.status, Status::Running);
}

fn question() -> (String, Value) {
    (
        "ask_user".into(),
        json!({"prompt":"Hold the parent boundary","input":{"kind":"text","multiline":true,"max_bytes":1024}}),
    )
}

async fn answer(fixture: &Fixture, question: QuestionId) {
    fixture
        .client
        .execute(fixture.client.prepare(Command::ResolveQuestion {
            session: fixture.session.id,
            question,
            response: question::Response::Answer(question::Answer::Text(
                "The parent tool is settled".into(),
            )),
        }))
        .await
        .unwrap();
}

#[tokio::test]
async fn wakes_foreground_and_batches_the_next_boundary() {
    for remote in [false, true] {
        let release = CancellationToken::new();
        let parent = Server::tools(vec![
            (
                "spawn_agent".into(),
                json!({"role":"review","task":"Hold the foreground inspection"}),
            ),
            question(),
        ])
        .await;
        let child = Server::markdown_held(
            "Foreground result survives steering".into(),
            release.clone(),
        )
        .await;
        let fixture = Fixture::start(remote, parent, child).await;
        let active = fixture.push("Delegate the foreground inspection").await;
        fixture.child.wait_count(1).await;
        let page = history(&fixture.client, fixture.session.id).await;
        assert_eq!(page.children.len(), 1);
        let child = page.children[0].run.clone();
        assert_live(&page, active.id, &child);
        assert_eq!(fixture.parent.requests.lock().unwrap().len(), 1);
        let wake = guide_parent(
            &fixture,
            &active,
            "Continue the parent while its child runs",
        )
        .await;
        let (page, question) = crate::questions::pending(&fixture.client, fixture.session.id).await;
        assert_live(&page, active.id, &child);
        assert_eq!(status(&page, wake.id), Status::Continued);
        let requests = fixture.parent.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert!(user_text(&requests[1]).contains("Continue the parent while its child runs"));
        assert!(
            !requests[1]["messages"]
                .to_string()
                .contains("Foreground result survives steering")
        );
        let texts = [
            "Steering boundary item one",
            "Steering boundary item two",
            "Steering boundary item three",
        ];
        let mut queued = Vec::new();
        for text in texts {
            queued.push(guide_parent(&fixture, &active, text).await);
        }
        assert_eq!(fixture.parent.requests.lock().unwrap().len(), 2);
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        let page = history(&fixture.client, fixture.session.id).await;
        assert_live(&page, active.id, &child);
        for input in &queued {
            assert_eq!(status(&page, input.id), Status::Queued);
        }
        answer(&fixture, question.id).await;
        fixture.parent.wait_count(3).await;
        let requests = fixture.parent.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 3);
        let delivered = user_text(&requests[2]);
        let mut previous = 0;
        for text in texts {
            assert_eq!(delivered.matches(text).count(), 1);
            let position = delivered.find(text).unwrap();
            assert!(position >= previous);
            previous = position + text.len();
        }
        let page = history(&fixture.client, fixture.session.id).await;
        assert_live(&page, active.id, &child);
        assert_eq!(
            status(&history(&fixture.client, child.session).await, child.turn),
            Status::Running
        );
        for input in &queued {
            let run = page.runs.iter().find(|run| run.turn == input.id).unwrap();
            assert_eq!(run.status, Status::Continued);
            assert_eq!(run.joined, Some(active.id));
        }
        assert!(!release.is_cancelled());
        release.cancel();
        fixture.parent.wait_count(4).await;
        let page = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(status(&page, active.id), Status::Completed);
        let child_page = finished(&fixture.client, child.session, child.turn).await;
        assert_eq!(status(&child_page, child.turn), Status::Completed);
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        received(
            &fixture.parent,
            child.session,
            "completed: Foreground result survives steering",
            1,
        );
        assert!(
            page.entries
                .iter()
                .filter(|entry| entry.author == "assistant")
                .all(|entry| entry.turn == active.id)
        );
        assert!(page.queue.items.is_empty());
        for (input, text) in queued.iter().zip(texts) {
            assert_eq!(
                page.entries
                    .iter()
                    .filter(|entry| entry.turn == input.id
                        && entry.author == "user"
                        && entry
                            .parts
                            .iter()
                            .any(|part| matches!(part, Part::Text(value) if value == text)))
                    .count(),
                1
            );
        }
        fixture.close().await;
    }
}
