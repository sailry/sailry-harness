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
    // Published snapshots keep one latest request per round.
    let rounds = observations
        .recent
        .iter()
        .map(|request| request.turn)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(rounds.len(), observations.recent.len());
    assert!(!observations.recent.is_empty());
    assert!(observations.recent.len() <= conversation::observation::RECENT_LIMIT);
    assert!(
        observations
            .recent
            .iter()
            .all(|request| request.attempts.len() == 1)
    );
    assert!(
        observations
            .recent
            .iter()
            .all(|request| request.model == "fixture-a" && request.context_limit == Some(128_000))
    );
    assert!(observations.latency.elapsed_us.is_some());
    let response = observations.latency.response_us.unwrap();
    assert!(response <= observations.latency.first_token_us.unwrap());
    assert!(response <= observations.latency.elapsed_us.unwrap());
    assert!(observations.latency.first_token_us.is_some());
}

async fn totals(view: &mut watch::Receiver<View>, turns: u64, responses: u64) {
    super::observed(view, |view| {
        assert!(
            view.statistics_error.is_none(),
            "{:#?}",
            view.statistics_error
        );
        view.statistics.as_ref().is_some_and(|statistics| {
            statistics.turns == turns
                && statistics.responses == responses
                && statistics.observations.calls == responses
                && statistics.observations.replies == turns
        })
    })
    .await;
    assert_totals(
        view.borrow().statistics.as_deref().unwrap(),
        turns,
        responses,
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

    /// Each update restarts the window, so slower hosts may take longer in total
    /// while a turn that stops publishing still fails.
    async fn wait(&mut self, turn: TurnId) {
        const WINDOW: Duration = Duration::from_secs(10);
        let deadline = tokio::time::Instant::now() + crate::ceiling(WINDOW);
        let mut updates = 0;
        let mut observed = None;
        loop {
            let limit = deadline.min(tokio::time::Instant::now() + WINDOW);
            let Ok(update) = tokio::time::timeout_at(limit, self.updates.next()).await else {
                let bound = if limit == deadline {
                    "overall ceiling"
                } else {
                    "progress window"
                };
                panic!(
                    "Agent completion hit its {bound} after {updates} updates; last run: {observed:?}"
                );
            };
            let update = update.unwrap();
            updates += 1;
            let applied = self.projection.apply(0, update).unwrap();
            assert_ne!(
                applied,
                sailry_client::Apply::Recover,
                "unexpected seed reset"
            );
            let run = self
                .projection
                .snapshot()
                .and_then(|snapshot| snapshot.page.runs.iter().find(|run| run.turn == turn));
            observed = run.map(|run| run.status);
            if let Some(run) = run
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
        let complete =
            tokio::time::timeout(WINDOW, self.client.read_conversation(self.session, None, 1))
                .await
                .expect("Agent completion deadline")
                .unwrap();
        assert!(complete.missing.is_empty());
        assert_eq!(complete.page.runs.len(), 1);
        let latest = &complete.page.runs[0];
        assert_eq!(latest.turn, turn);
        assert_eq!(latest.status, Status::Completed);
    }
}

#[tokio::test]
async fn restores_remote_streamed_turns() {
    restore_stream().await;
}

async fn restore_stream() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("project");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("资料.txt"), "History 中文 🙂").unwrap();
    let profile = fixture.path().join("node");
    let node = Node::start_empty(&profile).await.unwrap();
    let controller = Link::controller(fixture.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let transport = controller.handle().remote(address);
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
    let initial_entries = 2 * calls.len() + 2;
    let server = Server::tools(calls).await;
    let session = approvals::prepare_files(&client, &server, &root).await;
    // Keep all original tool output within the fixture context budget.
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
        initial_entries
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
    let (partial, requests) = mpsc::channel(1);
    let stop = CancellationToken::new();
    let worker = tokio::spawn({
        let stop = stop.clone();
        async move {
            hydrated
                .watch_conversation(session.id, updates, stop, requests)
                .await
        }
    });
    // The newest page publishes at once; the rest of the large turn loads on request.
    super::observed(&mut initial, |view| view.connected).await;
    {
        let view = initial.borrow();
        let snapshot = view.snapshot.as_ref().unwrap();
        assert_eq!(snapshot.missing, [first]);
        assert_eq!(snapshot.page.entries.len(), 100);
        assert_eq!(
            &snapshot.page.entries[..],
            &complete.page.entries[complete.page.entries.len() - 100..]
        );
    }
    partial.send(HistoryRequest::Older).await.unwrap();
    super::observed(&mut initial, |view| {
        !view.loading_older
            && view
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.missing.is_empty())
    })
    .await;
    assert_eq!(
        initial.borrow().snapshot.as_ref().unwrap().page.entries,
        complete.page.entries
    );
    assert_eq!(initial.borrow().calls.len(), 51);
    totals(&mut initial, 1, 52).await;
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
    totals(&mut view, 25, 76).await;
    let recent = view.borrow().snapshot.clone().unwrap();
    assert_eq!(recent.page.runs.len(), 20);
    assert_eq!(recent.page.next_before, Some(turns[5]));
    assert_eq!(recent.page.entries.len(), 40);
    assert_totals(recent.statistics.as_ref().unwrap(), 25, 76);
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
    totals(&mut view, 26, 77).await;
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
    // The oldest page leaves its large turn partial until another request.
    assert_eq!(view.borrow().snapshot.as_ref().unwrap().missing, [turns[0]]);
    older.send(HistoryRequest::Older).await.unwrap();
    super::observed(&mut view, |view| {
        !view.loading_older
            && view
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.missing.is_empty())
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
        initial_entries + 25 * 2
    );
    assert_totals(all.statistics.as_ref().unwrap(), 26, 77);
    assert_eq!(
        &all.page.entries[..complete.page.entries.len()],
        complete.page.entries.as_slice(),
        "streamed history preserves every original public entry"
    );
    assert_eq!(all.page.questions, complete.page.questions);
    assert_eq!(view.borrow().calls.len(), 51);
    assert_eq!(observed.cancelled_reads(), 0);
    // Automatic recovery keeps the loaded view connected and silent while it restores.
    observed.disconnect();
    assert!(view.borrow().connected);
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
    assert!(view.borrow().error.is_none());
    totals(&mut view, 27, 78).await;
    let all = view.borrow().snapshot.clone().unwrap();
    assert!(all.missing.is_empty());
    assert_eq!(all.page.next_before, None);
    assert_eq!(
        all.page
            .entries
            .iter()
            .filter(|entry| !is_context(entry))
            .count(),
        initial_entries + 26 * 2
    );
    assert_totals(all.statistics.as_ref().unwrap(), 27, 78);
    assert_eq!(
        all.page.as_ref(),
        &client
            .read_conversation(session.id, None, 100)
            .await
            .unwrap()
            .page
    );
    let mut expected_turns = turns.clone();
    expected_turns.extend([next, last]);
    let mut completion = Completion::new(&client, session.id).await;
    for _ in 0..24 {
        let turn = submit(&client, session.id).await;
        completion.wait(turn).await;
        expected_turns.push(turn);
    }
    drop(completion);
    let recent = client
        .read_conversation(session.id, None, 100)
        .await
        .unwrap();
    // The entry budget holds the newest 50 two-entry turns. The initial large
    // turn remains on the older page rather than appearing without its entries.
    assert_eq!(recent.page.runs.len(), 50);
    assert_eq!(recent.page.next_before, Some(expected_turns[1]));
    assert!(recent.missing.is_empty());
    let older_page = client
        .read_conversation(session.id, recent.page.next_before, 100)
        .await
        .unwrap();
    assert_eq!(older_page.page.runs.len(), 1);
    assert!(older_page.page.next_before.is_none());
    assert!(older_page.missing.is_empty());
    let complete_turns: Vec<_> = older_page
        .page
        .runs
        .iter()
        .chain(&recent.page.runs)
        .map(|run| run.turn)
        .collect();
    assert_eq!(complete_turns, expected_turns);
    assert_eq!(complete_turns.len(), 51);
    assert_eq!(
        complete_turns
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        51
    );
    let statistics = observations::statistics(&client, session.id).await;
    assert_totals(&statistics, 51, 102);
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
                .filter(|request| {
                    !agent_support::compaction::is_summary(request)
                        && !agent_support::compaction::is_tool_pair(request)
                })
                .count(),
            102
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| agent_support::compaction::is_tool_pair(request))
                .count(),
            0
        );
        requests.len()
    };
    stop.cancel();
    worker.await.unwrap().unwrap();
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
    let node = Node::start_empty(&profile).await.unwrap();
    let client = Client::new(node.local());
    let restored = observations::statistics(&client, session.id).await;
    assert_totals(&restored, 51, 102);
    assert_eq!(restored, statistics);
    assert_eq!(
        client
            .read_conversation(session.id, None, 100)
            .await
            .unwrap()
            .page,
        recent.page
    );
    assert_eq!(
        client
            .read_conversation(session.id, recent.page.next_before, 100)
            .await
            .unwrap()
            .page,
        older_page.page
    );
    assert_eq!(server.requests.lock().unwrap().len(), model_requests);
    node.shutdown().await.unwrap();
}
