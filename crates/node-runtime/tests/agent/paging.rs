use super::*;
use sailry_client::conversation::{HistoryRequest, View};
use sailry_link::CancellationToken;
use serde_json::json;
use tokio::sync::{mpsc, watch};

#[path = "paging/details.rs"]
mod details;
#[path = "paging/entries.rs"]
mod entries;
#[path = "paging/removal.rs"]
mod removal;
#[path = "paging/seek.rs"]
mod seek;
#[path = "paging/transport.rs"]
pub(super) mod transport;
use transport::Observed;

fn assert_totals(statistics: &Statistics, turns: u64, responses: u64) {
    assert_eq!(statistics.turns, turns);
    assert_eq!(statistics.responses, responses);
    assert_eq!(statistics.context_tokens, Some(16));
    assert_eq!(statistics.context_estimate, None);
    assert_eq!(statistics.context_limit, Some(128_000));
    assert_eq!(statistics.context_model.as_deref(), Some("fixture-a"));
    assert!(statistics.context_timestamp_ms.is_some());
    assert_eq!(
        statistics.usage,
        Some(Usage {
            input: responses * 12,
            output: responses * 4,
            cached_input: 0,
            reasoning: 0,
        })
    );
    assert!(statistics.first_token_us.is_some());
    assert_eq!(statistics.cost, None);
    assert_eq!(
        statistics.delegated,
        sailry_protocol::usage::Metrics::default()
    );
    assert_eq!(statistics.last_compaction, None);
    let generation = statistics.generation.as_ref().unwrap();
    assert_eq!(generation.responses, responses);
    assert_eq!(generation.output_tokens, responses * 4);
    let budget = statistics.budget.as_ref().unwrap();
    assert_eq!(budget.model, "fixture-a");
    assert_eq!(budget.context_limit, 128_000);
    let observations = &statistics.observations;
    assert_eq!(observations.calls, responses);
    assert_eq!(observations.replies, turns);
    assert_eq!(observations.compactions, 0);
    assert_eq!(
        observations.recent.len(),
        (responses as usize).min(conversation::observation::RECENT_LIMIT)
    );
    let ids = observations
        .recent
        .iter()
        .map(|request| &request.id)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), observations.recent.len());
    assert!(
        observations
            .recent
            .iter()
            .all(|request| request.model == "fixture-a" && request.context_limit == Some(128_000))
    );
    assert!(observations.latency.elapsed_us.is_some());
    assert!(observations.latency.response_us.is_some());
    assert!(observations.latency.first_text_us.is_some());
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

struct Completion<'a> {
    client: &'a Client,
    session: SessionId,
    updates: Box<dyn sailry_link::Subscription>,
    projection: sailry_client::conversation::Projection,
}

impl<'a> Completion<'a> {
    async fn new(client: &'a Client, session: SessionId) -> Self {
        Self {
            client,
            session,
            updates: client.subscribe_conversation(session).await.unwrap(),
            projection: sailry_client::conversation::Projection::new(client.target(), session, 0),
        }
    }

    async fn wait(&mut self, turn: TurnId) {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let applied = self
                    .projection
                    .apply(0, self.updates.next().await.unwrap())
                    .unwrap();
                assert_ne!(
                    applied,
                    sailry_client::Apply::Recover,
                    "unexpected seed reset"
                );
                if let Some(run) = self
                    .projection
                    .snapshot()
                    .and_then(|snapshot| snapshot.page.runs.iter().find(|run| run.turn == turn))
                    && !matches!(
                        run.status,
                        Status::Queued | Status::Running | Status::Stopping
                    )
                {
                    assert_eq!(run.status, Status::Completed, "{run:?}");
                    break;
                }
            }
            // The terminal frame can precede later entry frames in the same commit.
            let complete = self
                .client
                .read_conversation(self.session, None, 1)
                .await
                .unwrap();
            assert!(complete.missing.is_empty());
            assert_eq!(complete.page.runs.len(), 1);
            let latest = &complete.page.runs[0];
            assert_eq!(latest.turn, turn);
            assert_eq!(latest.status, Status::Completed);
        })
        .await
        .expect("Agent completion deadline");
    }
}

#[tokio::test]
async fn restores_local_streamed_turns() {
    restore_stream(false).await;
}

#[tokio::test]
async fn restores_remote_streamed_turns() {
    restore_stream(true).await;
}

async fn restore_stream(remote: bool) {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("project");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("资料.txt"), "History 中文 🙂").unwrap();
    let profile = fixture.path().join("node");
    let node = Node::start_empty(&profile).await.unwrap();
    let controller = if remote {
        Some(
            Link::controller(fixture.path().join("controller"), NetworkScope::default())
                .await
                .unwrap(),
        )
    } else {
        None
    };
    let transport = if let Some(controller) = &controller {
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
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
    let session = approvals::prepare_files(&client, &server, &root).await;
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
    let finished = finished_within(&client, session.id, first, Duration::from_secs(30)).await;
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
    assert_eq!(
        complete
            .page
            .entries
            .iter()
            .filter(|entry| !is_context(entry))
            .count(),
        155
    );
    assert_eq!(
        complete
            .page
            .entries
            .iter()
            .filter(|entry| is_context(entry))
            .count(),
        1
    );
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
            assert_eq!(
                snapshot
                    .page
                    .entries
                    .iter()
                    .filter(|entry| !is_context(entry))
                    .count(),
                155
            );
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
    let mut completion = Completion::new(&client, session.id).await;
    for _ in 0..24 {
        let turn = submit(&client, session.id).await;
        completion.wait(turn).await;
        turns.push(turn);
    }
    drop(completion);
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
    assert_eq!(
        all.page
            .entries
            .iter()
            .filter(|entry| !is_context(entry))
            .count(),
        205
    );
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
    assert_eq!(
        all.page
            .entries
            .iter()
            .filter(|entry| !is_context(entry))
            .count(),
        207
    );
    assert_totals(&all.statistics, 27, 78);
    assert_eq!(
        all.page.as_ref(),
        &client
            .read_conversation(session.id, None, 100)
            .await
            .unwrap()
            .page
    );
    let mut completion = Completion::new(&client, session.id).await;
    for _ in 0..24 {
        let turn = submit(&client, session.id).await;
        completion.wait(turn).await;
    }
    drop(completion);
    let all = client
        .read_conversation(session.id, None, 100)
        .await
        .unwrap();
    assert_eq!(all.page.runs.len(), 51);
    let statistics = {
        let mut updates = client.subscribe_conversation(session.id).await.unwrap();
        let Update::ConversationSnapshot(snapshot) = updates.next().await.unwrap() else {
            panic!("snapshot expected");
        };
        assert_totals(&snapshot.statistics, 51, 102);
        snapshot.statistics
    };
    for format in [
        sailry_client::conversation::export::Format::Markdown,
        sailry_client::conversation::export::Format::Html,
    ] {
        let document = client
            .export_conversation(
                session.id,
                "History export",
                format,
                "export-assets",
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(document.content.matches("History fixture").count(), 51);
        assert_eq!(document.content.matches("answer-fixture-a").count(), 51);
        assert!(!document.content.contains("原文 🙂"));
        assert!(!document.content.contains("资料.txt"));
        assert!(document.attachments.is_empty());
    }
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert_eq!(
        client
            .export_conversation(
                session.id,
                "Cancelled",
                sailry_client::conversation::export::Format::Markdown,
                "assets",
                &cancelled
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    let model_requests = {
        let requests = server.requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| !agent_support::compaction::is_summary(request))
                .count(),
            102
        );
        requests.len()
    };
    stop.cancel();
    worker.await.unwrap().unwrap();
    if let Some(controller) = controller {
        controller.close().await.unwrap();
    }
    node.shutdown().await.unwrap();
    let node = Node::start_empty(&profile).await.unwrap();
    let client = Client::new(node.local());
    let mut subscription = client.subscribe_conversation(session.id).await.unwrap();
    let Update::ConversationSnapshot(restored) = subscription.next().await.unwrap() else {
        panic!("snapshot expected");
    };
    assert_totals(&restored.statistics, 51, 102);
    assert_eq!(restored.statistics, statistics);
    drop(subscription);
    assert_eq!(
        client
            .read_conversation(session.id, None, 100)
            .await
            .unwrap()
            .page,
        all.page
    );
    assert_eq!(server.requests.lock().unwrap().len(), model_requests);
    node.shutdown().await.unwrap();
}
