//! Neutral declarations exercise Node scheduling without a bundled package dependency.
use super::*;
use sailry_link::CancellationToken;

mod fixture;
mod observation;
use fixture::Fixture;

fn status(page: &Page, turn: TurnId) -> Status {
    page.runs
        .iter()
        .find(|run| run.turn == turn)
        .expect("run expected")
        .status
}

async fn visible(client: &Client, run: &Run, expected: &sailry_protocol::conversation::Input) {
    let page = history(client, run.session).await;
    assert_eq!(status(&page, run.turn), run.status);
    let input = sailry_client::conversation::turns::input(&page, run.turn).unwrap();
    assert_eq!(&input.message, expected);
    let captured = input.message.clone();
    let mut updates = client.subscribe_conversation(run.session).await.unwrap();
    let mut projection =
        sailry_client::conversation::Projection::new(client.target(), run.session, 0);
    assert_eq!(
        projection.apply(0, updates.next().await.unwrap()).unwrap(),
        sailry_client::Apply::Applied
    );
    assert_eq!(
        status(&projection.snapshot().unwrap().page, run.turn),
        run.status
    );
    assert_eq!(
        sailry_client::conversation::turns::input(&projection.snapshot().unwrap().page, run.turn)
            .unwrap()
            .message,
        captured
    );
}

#[tokio::test]
async fn isolates_parent_budgets() {
    for remote in [false, true] {
        let release = CancellationToken::new();
        let child = Server::markdown_held("Frozen child output".into(), release.clone()).await;
        let mut fixture = Fixture::start(remote, true, child, 3).await;
        fixture.capacity(1).await;
        let first = fixture.push(0).await;
        assert_eq!(
            status(
                &finished(&fixture.client, fixture.parents[0].id, first.id).await,
                first.id
            ),
            Status::Completed
        );
        fixture.wait(3, 1).await;
        let second = fixture.push(1).await;
        for (index, turn) in [(0, first), (1, second)] {
            let page = finished(&fixture.client, fixture.parents[index].id, turn.id).await;
            assert_eq!(status(&page, turn.id), Status::Completed);
            assert!(
                results(&page)
                    .iter()
                    .all(|value| value["status"] == "queued")
            );
        }
        let original = fixture.wait(6, 2).await;
        for group in original.as_chunks::<3>().0 {
            assert_eq!(group[0].status, Status::Running);
            assert!(group[1..].iter().all(|run| run.status == Status::Queued));
        }
        let mut inputs = std::collections::BTreeMap::new();
        for parent in &fixture.parents {
            let page = history(&fixture.client, parent.id).await;
            for child in &page.children {
                assert_eq!(child.origin.session, parent.id);
                let entry = page
                    .entries
                    .iter()
                    .find(|entry| entry.id == child.origin.entry)
                    .unwrap();
                let Part::ToolCall { arguments, .. } = &entry.parts[child.origin.index] else {
                    panic!("origin call expected")
                };
                let input =
                    sailry_protocol::conversation::Input::from(arguments["task"].as_str().unwrap());
                assert!(inputs.insert(child.run.turn, input).is_none());
            }
        }
        assert_eq!(inputs.len(), original.len());
        for run in &original {
            visible(&fixture.client, run, &inputs[&run.turn]).await;
        }
        fixture.child.wait_count(2).await;
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 2);
        let mut captures = Vec::new();
        for run in &original {
            captures.push(
                fixture
                    .client
                    .read_turn_capture(run.session, run.turn)
                    .await
                    .unwrap(),
            );
        }

        // Updating current configuration cannot replace an already queued capture.
        fixture.provider.endpoint = "http://127.0.0.1:1/v1".into();
        fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                provider: fixture.provider.clone(),
                expected_revision: fixture.provider.revision,
            }))
            .await
            .unwrap();
        fixture.role.instructions = "Replacement role instructions".into();
        fixture
            .client
            .execute(fixture.client.prepare(Command::PutRole {
                role: fixture.role.clone(),
                expected_revision: fixture.role.revision,
            }))
            .await
            .unwrap();
        fixture.upgrade().await;
        fixture.capacity(2).await;
        let expanded = fixture.wait(6, 4).await;
        for group in expanded.as_chunks::<3>().0 {
            assert!(group[..2].iter().all(|run| run.status == Status::Running));
            assert_eq!(group[2].status, Status::Queued);
        }
        for run in &expanded {
            visible(&fixture.client, run, &inputs[&run.turn]).await;
        }
        fixture.child.wait_count(4).await;
        fixture.capacity(1).await;
        let lowered = fixture.runs().await;
        assert_eq!(
            lowered
                .iter()
                .filter(|run| run.status == Status::Running)
                .count(),
            4
        );
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 4);
        release.cancel();
        for run in &original {
            let page = finished(&fixture.client, run.session, run.turn).await;
            assert_eq!(
                sailry_client::conversation::turns::input(&page, run.turn)
                    .unwrap()
                    .message,
                inputs[&run.turn]
            );
            assert_eq!(status(&page, run.turn), Status::Completed);
            let completed = page
                .runs
                .iter()
                .find(|value| value.turn == run.turn)
                .unwrap();
            visible(&fixture.client, completed, &inputs[&run.turn]).await;
        }
        for capture in captures {
            assert_eq!(
                fixture
                    .client
                    .read_turn_capture(capture.session, capture.id)
                    .await
                    .unwrap(),
                capture
            );
        }
        let requests = fixture.child.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 6);
        for request in requests {
            let text = request["messages"].to_string();
            assert!(text.contains("Frozen capacity instructions"));
            assert!(!text.contains("Replacement role instructions"));
        }
        fixture.close().await;
    }
}

#[tokio::test]
async fn cancels_waiters_without_replaying() {
    for remote in [false, true] {
        let child =
            Server::markdown_held("Unreleased output".into(), CancellationToken::new()).await;
        let mut fixture = Fixture::start(remote, false, child, 2).await;
        fixture.capacity(1).await;
        let parent = fixture.push(0).await;
        let runs = fixture.wait(2, 1).await;
        assert_eq!(
            status(
                &history(&fixture.client, fixture.parents[0].id).await,
                parent.id
            ),
            Status::Running
        );
        assert!(results(&history(&fixture.client, fixture.parents[0].id).await).is_empty());
        let queued = runs
            .iter()
            .find(|run| run.status == Status::Queued)
            .unwrap();
        fixture.child.wait_count(1).await;
        fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::StopTurn { turn: queued.turn }),
            )
            .await
            .unwrap();
        assert_eq!(
            status(
                &finished(&fixture.client, queued.session, queued.turn).await,
                queued.turn
            ),
            Status::Cancelled
        );
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        // A second root is free to admit work while the first synchronously waits.
        let second = fixture.push(1).await;
        fixture.wait(4, 2).await;
        fixture.child.wait_count(2).await;
        fixture = fixture.restart().await;
        for run in fixture.runs().await {
            let expected = if run.turn == queued.turn {
                Status::Cancelled
            } else {
                Status::Interrupted
            };
            assert_eq!(run.status, expected);
        }
        for (index, turn) in [(0, parent), (1, second)] {
            assert_eq!(
                status(
                    &history(&fixture.client, fixture.parents[index].id).await,
                    turn.id
                ),
                Status::Interrupted
            );
        }
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 2);
        fixture.close().await;
    }
}

#[tokio::test]
async fn failures_release_slots() {
    for remote in [false, true] {
        let fixture = Fixture::start(remote, false, Server::http(usize::MAX, 400).await, 2).await;
        fixture.capacity(1).await;
        let turn = fixture.push(0).await;
        let page = finished(&fixture.client, fixture.parents[0].id, turn.id).await;
        assert_eq!(status(&page, turn.id), Status::Completed);
        assert_eq!(
            page.children.len(),
            2,
            "tool results: {:?}; declared tools: {:?}",
            results(&page),
            fixture
                .parent
                .requests
                .lock()
                .unwrap()
                .first()
                .map(|request| request["tools"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|tool| tool["function"]["name"].clone())
                    .collect::<Vec<_>>())
        );
        assert!(
            page.children
                .iter()
                .all(|child| child.run.status == Status::Failed)
        );
        assert_eq!(
            fixture.child.requests.lock().unwrap().len(),
            2,
            "{:?}",
            page.children
                .iter()
                .map(|child| &child.run.error)
                .collect::<Vec<_>>()
        );
        fixture.close().await;
    }
}
