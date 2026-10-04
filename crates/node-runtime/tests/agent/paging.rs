use super::*;
use sailry_client::conversation::{HistoryRequest, View};
use sailry_link::CancellationToken;
use serde_json::json;
use tokio::sync::{mpsc, watch};

#[path = "paging/seek.rs"]
mod seek;
#[path = "paging/transport.rs"]
pub(super) mod transport;
use transport::Observed;

fn assert_totals(statistics: &Statistics, turns: u64, responses: u64) {
    assert_eq!(
        statistics,
        &Statistics {
            turns,
            responses,
            context_tokens: Some(16),
            usage: Some(Usage {
                input: responses * 12,
                output: responses * 4,
                cached_input: 0,
                reasoning: 0
            }),
            generation: statistics.generation.clone(),
            ..Default::default()
        }
    );
}

async fn submit(client: &Client, session: SessionId) -> TurnId {
    let Output::QueuedTurn(turn) = client
        .execute(client.prepare(Command::SubmitTurn {
            session,
            expected_revision: 1,
            message: "History fixture".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    turn.id
}

#[tokio::test]
async fn restores_streamed_turns() {
    for remote in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("资料.txt"), "History 中文 🙂").unwrap();
        let profile = fixture.path().join("node");
        let node = Node::start(&profile).await.unwrap();
        let controller =
            Link::controller(fixture.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let transport = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let client = Client::new(transport.clone());
        let mut calls = vec![(
            "ask_user".into(),
            json!({"prompt": "History question", "input": {"kind": "text", "multiline": false, "max_bytes": 100}}),
        )];
        calls.extend((0..50).map(|_| {
            (
                plugin_tool("files", "read_file"),
                json!({"path": "资料.txt"}),
            )
        }));
        let server = Server::tools(calls).await;
        let session = approvals::prepare(&client, &server, &root).await;
        // This fixture verifies exact paging/call counts, not automatic summaries.
        // Keep all 51 tool exchanges below the model's compaction threshold.
        let Output::Providers(mut providers) = client
            .execute(client.prepare(Command::ListProviders))
            .await
            .unwrap()
        else {
            panic!("providers expected")
        };
        let mut provider = providers.remove(0);
        provider.models[0].context = 128_000;
        client
            .execute(client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap();
        let first = submit(&client, session.id).await;
        let mut pending = client.subscribe_conversation(session.id).await.unwrap();
        let mut projection =
            sailry_client::conversation::Projection::new(client.target(), session.id, 0);
        let question = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                projection.apply(0, pending.next().await.unwrap()).unwrap();
                if let Some(question) = projection.snapshot().unwrap().page.questions.first() {
                    break question.clone();
                }
            }
        })
        .await
        .unwrap();
        client
            .execute(client.prepare(Command::ResolveQuestion {
                session: session.id,
                question: question.id,
                response: question::Response::Answer(question::Answer::Text("原文 🙂".into())),
            }))
            .await
            .unwrap();
        let finished = finished(&client, session.id, first).await;
        assert_eq!(
            finished.runs[0].status,
            Status::Completed,
            "{:?}",
            finished.runs
        );
        drop(pending);
        let Output::Conversation(raw) = client
            .execute(client.prepare(Command::ReadConversation {
                session: session.id,
                before: None,
                limit: 1,
            }))
            .await
            .unwrap()
        else {
            panic!("history expected")
        };
        assert_eq!(raw.missing, [first]);
        assert_eq!(raw.page.entries.len(), 100);
        let complete = client.read_conversation(session.id, None, 1).await.unwrap();
        assert!(complete.missing.is_empty());
        assert_eq!(complete.page.entries.len(), 155);
        assert_eq!(
            complete.page.questions,
            [question::Question {
                state: question::State::Answered(question::Answer::Text("原文 🙂".into())),
                ..question
            }]
        );
        assert_eq!(
            complete
                .page
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter(|part| matches!(part, Part::ToolCall { .. }))
                .count(),
            51
        );
        assert_eq!(server.requests.lock().unwrap().len(), 52);
        let hydrated = Arc::new(Client::new(transport.clone()));
        let (updates, mut initial) = watch::channel(View::default());
        let (_older, requests) = mpsc::channel(1);
        let stop = CancellationToken::new();
        let worker = tokio::spawn({
            let stop = stop.clone();
            async move {
                hydrated
                    .watch_conversation(session.id, updates, stop, requests)
                    .await
            }
        });
        super::observed(&mut initial, |view| {
            if let Some(snapshot) = &view.snapshot {
                assert!(snapshot.missing.is_empty());
                assert_eq!(snapshot.page.entries.len(), 155);
            }
            view.connected
        })
        .await;
        assert_eq!(initial.borrow().calls.len(), 51);
        assert_totals(
            &initial.borrow().snapshot.as_ref().unwrap().statistics,
            1,
            52,
        );
        stop.cancel();
        worker.await.unwrap().unwrap();
        let mut turns = vec![first];
        for _ in 0..24 {
            let turn = submit(&client, session.id).await;
            super::finished(&client, session.id, turn).await;
            turns.push(turn);
        }

        let observed = Arc::new(Observed::new(transport));
        let watching = Arc::new(Client::new(observed.clone()));
        let (updates, mut view) = watch::channel(View::default());
        let (older, requests) = mpsc::channel(8);
        let stop = CancellationToken::new();
        let worker = tokio::spawn({
            let watching = watching.clone();
            let stop = stop.clone();
            async move {
                watching
                    .watch_conversation(session.id, updates, stop, requests)
                    .await
            }
        });
        super::observed(&mut view, |view| view.connected).await;
        let recent = view.borrow().snapshot.clone().unwrap();
        assert_eq!(recent.page.runs.len(), 20);
        assert_eq!(recent.page.next_before, Some(turns[5]));
        assert_eq!(recent.page.entries.len(), 40);
        assert_totals(&recent.statistics, 25, 76);
        older.send(HistoryRequest::Older).await.unwrap();
        super::observed(&mut view, |view| view.older_error.is_some()).await;
        assert!(view.borrow().connected);
        assert!(!view.borrow().loading_older);
        assert_eq!(view.borrow().snapshot.as_ref().unwrap(), &recent);
        assert_eq!(observed.reads(), 1);
        older.send(HistoryRequest::Older).await.unwrap();
        super::observed(&mut view, |view| view.loading_older).await;
        observed.wait_held().await;
        for _ in 0..4 {
            older.send(HistoryRequest::Older).await.unwrap();
        }
        let next = submit(&client, session.id).await;
        super::observed(&mut view, |view| {
            view.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot
                    .page
                    .runs
                    .last()
                    .is_some_and(|run| run.turn == next && run.status == Status::Completed)
            })
        })
        .await;
        assert!(view.borrow().loading_older);
        assert_eq!(view.borrow().snapshot.as_ref().unwrap().page.runs.len(), 21);
        assert_totals(&view.borrow().snapshot.as_ref().unwrap().statistics, 26, 77);
        assert_eq!(observed.cancelled_reads(), 0);
        observed.release();
        super::observed(&mut view, |view| {
            !view.loading_older
                && view
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.page.next_before.is_none())
        })
        .await;
        assert_eq!(observed.reads(), 2);
        let all = view.borrow().snapshot.clone().unwrap();
        assert!(all.missing.is_empty());
        assert_eq!(all.page.runs.len(), 26);
        assert_eq!(all.page.entries.len(), 205);
        assert_totals(&all.statistics, 26, 77);
        assert_eq!(all.page.questions, complete.page.questions);
        assert_eq!(view.borrow().calls.len(), 51);
        assert_eq!(observed.cancelled_reads(), 0);
        observed.disconnect();
        super::observed(&mut view, |view| !view.connected).await;
        assert_eq!(view.borrow().snapshot.as_ref().unwrap(), &all);
        let last = submit(&client, session.id).await;
        super::finished(&client, session.id, last).await;
        super::observed(&mut view, |view| {
            view.connected
                && view.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.page.runs.len() == 27
                        && snapshot
                            .page
                            .runs
                            .last()
                            .is_some_and(|run| run.turn == last && run.status == Status::Completed)
                })
        })
        .await;
        let all = view.borrow().snapshot.clone().unwrap();
        assert!(all.missing.is_empty());
        assert_eq!(all.page.next_before, None);
        assert_eq!(all.page.entries.len(), 207);
        assert_totals(&all.statistics, 27, 78);
        assert_eq!(
            all.page.as_ref(),
            &client
                .read_conversation(session.id, None, 100)
                .await
                .unwrap()
                .page
        );
        let model_requests = {
            let requests = server.requests.lock().unwrap();
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| !agent_support::compaction::is_summary(request))
                    .count(),
                78
            );
            requests.len()
        };
        stop.cancel();
        worker.await.unwrap().unwrap();
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        let node = Node::start(&profile).await.unwrap();
        let client = Client::new(node.local());
        let mut subscription = client.subscribe_conversation(session.id).await.unwrap();
        let Update::ConversationSnapshot(restored) = subscription.next().await.unwrap() else {
            panic!("snapshot expected");
        };
        assert_eq!(restored.statistics, all.statistics);
        drop(subscription);
        assert_eq!(
            client
                .read_conversation(session.id, None, 100)
                .await
                .unwrap()
                .page,
            *all.page
        );
        assert_eq!(server.requests.lock().unwrap().len(), model_requests);
        node.shutdown().await.unwrap();
    }
}
