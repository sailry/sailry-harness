use super::*;
use sailry_protocol::conversation::observation::{Attempt, CategoryKind, Outcome};

fn attempt(index: u64, outcome: Outcome) -> Attempt {
    Attempt {
        index,
        transport: "http".into(),
        reused: Some(false),
        headers_us: Some(2),
        response_us: Some(2),
        first_text_us: Some(4),
        elapsed_us: Some(10),
        status: Some(if outcome == Outcome::Failed { 503 } else { 200 }),
        outcome,
        categories: vec![Category {
            kind: CategoryKind::User,
            items: Some(1),
            tokens: Some(3),
            method: Some("estimate".into()),
        }],
        complete_context: true,
        usage: Some(Usage {
            input: 10,
            output: 4,
            cached_input: 2,
            reasoning: 1,
        }),
        cache_reported: true,
        reasoning_reported: true,
    }
}

fn observed(request: &Request) -> canonical::Record {
    let mut event = filler();
    event
        .metadata
        .execution
        .get_or_insert_with(Default::default)
        .observation = Some(request.clone());
    event
}

#[test]
fn averages_valid_samples() {
    let base = attempt(1, Outcome::Succeeded);
    let samples = [
        Attempt {
            response_us: None,
            headers_us: Some(0),
            ..base.clone()
        },
        Attempt {
            transport: "web_socket".into(),
            headers_us: Some(999),
            response_us: Some(2),
            first_text_us: Some(6),
            elapsed_us: Some(20),
            ..base.clone()
        },
        Attempt {
            transport: "web_socket".into(),
            headers_us: Some(3),
            response_us: None,
            first_text_us: Some(8),
            elapsed_us: Some(30),
            ..base.clone()
        },
        Attempt {
            response_us: Some(11),
            first_text_us: Some(5),
            ..base.clone()
        },
        Attempt {
            response_us: Some(3),
            first_text_us: Some(12),
            ..base.clone()
        },
        Attempt {
            headers_us: None,
            response_us: None,
            first_text_us: None,
            elapsed_us: None,
            ..base.clone()
        },
        Attempt {
            response_us: Some(8),
            first_text_us: Some(6),
            ..base.clone()
        },
        Attempt {
            headers_us: Some(0),
            response_us: Some(0),
            first_text_us: Some(0),
            elapsed_us: Some(0),
            ..base
        },
    ];
    let mut latency = Latencies::default();
    for sample in &samples {
        latency.observe(sample);
    }
    assert_eq!(
        latency.finish(),
        Latency {
            response_us: Some(2),
            first_text_us: Some(5),
            waiting_us: Some(4),
            elapsed_us: Some(12),
        }
    );
    let mut delivered = Latencies::default();
    delivered.observe(&samples[4]);
    assert_eq!(
        delivered.finish(),
        Latency {
            response_us: Some(3),
            first_text_us: Some(12),
            waiting_us: Some(9),
            elapsed_us: Some(10),
        }
    );
    let mut unordered = Latencies::default();
    unordered.observe(&samples[6]);
    assert_eq!(unordered.finish().waiting_us, None);
    let mut invalid = Latencies::default();
    invalid.observe(&samples[3]);
    assert_eq!(
        invalid.finish(),
        Latency {
            response_us: None,
            first_text_us: Some(5),
            waiting_us: None,
            elapsed_us: Some(10),
        }
    );
    let mut unknown = Latencies::default();
    unknown.observe(&samples[5]);
    assert_eq!(unknown.finish(), Latency::default());
    let mut large = Mean::default();
    large.observe(Some(u64::MAX));
    large.observe(Some(u64::MAX));
    assert_eq!(large.finish(), Some(u64::MAX));
}

#[tokio::test]
async fn averages_history_before_limiting_requests() {
    let mut fixture = Fixture::new().await;
    let turn = fixture.queued().await;
    let events = (0..RECENT_LIMIT + 2)
        .map(|index| {
            let mut sample = attempt(1, Outcome::Succeeded);
            let scale = if index == 0 { 6600 } else { 0 };
            sample.headers_us = Some(scale);
            sample.response_us = Some(scale);
            sample.first_text_us = Some(scale * 2);
            sample.elapsed_us = Some(scale * 3);
            if index == 0 {
                sample.outcome = Outcome::Failed;
            }
            observed(&Request {
                id: RequestId::new().to_string(),
                turn,
                turn_index: None,
                model: "fixture".into(),
                context_limit: None,
                timestamp_ms: 1,
                boundary: None,
                attempts: vec![sample],
            })
        })
        .chain([response()]);
    complete(&mut fixture, turn, events).await;
    let summary = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(summary.calls, 66);
    assert_eq!(summary.recent.len(), RECENT_LIMIT);
    assert!(
        summary
            .recent
            .iter()
            .flat_map(|request| &request.attempts)
            .all(|attempt| attempt.elapsed_us == Some(0))
    );
    assert_eq!(
        summary.latency,
        Latency {
            response_us: Some(100),
            first_text_us: Some(200),
            waiting_us: Some(100),
            elapsed_us: Some(300),
        }
    );
}

#[tokio::test]
async fn merges_recovered_attempts_across_rebuilds() {
    let mut fixture = Fixture::new().await;
    let turn = fixture.queued().await;
    let request = Request {
        id: RequestId::new().to_string(),
        turn,
        turn_index: None,
        model: "fixture".into(),
        context_limit: None,
        timestamp_ms: 1,
        boundary: None,
        attempts: Vec::new(),
    };
    let mut active = request.clone();
    active.attempts = vec![Attempt {
        elapsed_us: None,
        usage: None,
        ..attempt(1, Outcome::Cancelled)
    }];
    let mut failed = request.clone();
    failed.attempts = vec![attempt(1, Outcome::Failed)];
    let failed_event = observed(&failed);
    let mut cancelled = request.clone();
    cancelled.attempts = vec![attempt(2, Outcome::Cancelled)];
    let mut succeeded = request.clone();
    succeeded.attempts = vec![attempt(3, Outcome::Succeeded)];
    let mut snapshot = request.clone();
    snapshot.attempts = vec![attempt(1, Outcome::Failed), attempt(2, Outcome::Cancelled)];
    let phases = Phases {
        active_us: Some(30),
        waiting_us: Some(12),
        output_us: Some(18),
        tools_us: Some(0),
        other_us: Some(0),
    };
    complete(
        &mut fixture,
        turn,
        [
            observed(&request),
            observed(&active),
            failed_event.clone(),
            observed(&cancelled),
            observed(&snapshot),
            observed(&succeeded),
            observed(&active),
            response(),
            phase(phases.clone()),
        ],
    )
    .await;
    let before: i64 = fixture
        .database
        .connection
        .prepare("SELECT count(*) FROM agent_events WHERE turn=?1")
        .await
        .unwrap()
        .query_row([turn.to_string()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        failed_event,
        &fixture.events,
    )
    .await
    .unwrap();
    let after: i64 = fixture
        .database
        .connection
        .prepare("SELECT count(*) FROM agent_events WHERE turn=?1")
        .await
        .unwrap()
        .query_row([turn.to_string()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    assert_eq!(
        before, after,
        "an acknowledged terminal event is not appended twice"
    );
    let expected = super::super::super::read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let capture: Vec<u8> = fixture
        .database
        .connection
        .prepare("SELECT input FROM agent_runs WHERE turn=?1")
        .await
        .unwrap()
        .query_row([turn.to_string()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    assert_eq!(expected.responses, 1);
    assert_eq!(
        expected.usage,
        Some(Usage {
            input: 10,
            output: 4,
            cached_input: 2,
            reasoning: 1
        })
    );
    let summary = &expected.observations;
    assert_eq!(summary.calls, 1);
    assert_eq!(summary.replies, 1);
    assert_eq!(summary.phases, phases);
    assert_eq!(summary.model_us, 30);
    assert_eq!(summary.output_us, 18);
    assert_eq!(summary.output_tokens, 9);
    assert_eq!(summary.output_samples, 3);
    assert_eq!(
        summary.latency,
        Latency {
            response_us: Some(2),
            first_text_us: Some(4),
            waiting_us: Some(2),
            elapsed_us: Some(10),
        }
    );
    assert_eq!(summary.composition[0].items, Some(3));
    assert_eq!(summary.composition[0].tokens, Some(9));
    let call = &summary.recent[0];
    assert_eq!(call.id, request.id);
    assert_eq!(call.turn_index, Some(1));
    assert_eq!(
        call.attempts,
        vec![
            attempt(1, Outcome::Failed),
            attempt(2, Outcome::Cancelled),
            attempt(3, Outcome::Succeeded)
        ]
    );
    let page = fixture.read(None, 20).await.page;
    assert!(page.entries.iter().any(|entry| {
        entry
            .parts
            .iter()
            .any(|part| matches!(part, Part::Text(text) if text == "Response"))
    }));
    assert!(page.entries.iter().all(|entry| !entry.parts.is_empty()));
    let source = fixture.session;
    let fork = fixture.fork(source, turn).await;
    let inherited = super::super::super::read(&fixture.database.connection, fork.id)
        .await
        .unwrap();
    assert_eq!(inherited, expected);
    history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    let rebuilt: Vec<u8> = fixture
        .database
        .connection
        .prepare("SELECT input FROM agent_runs WHERE turn=?1")
        .await
        .unwrap()
        .query_row([turn.to_string()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    assert_eq!(rebuilt, capture);
    for session in [source, fork.id] {
        assert_eq!(
            super::super::super::read(&fixture.database.connection, session)
                .await
                .unwrap(),
            expected
        );
    }
    assert_eq!(fixture.read(None, 20).await.page, page);
}

#[tokio::test]
async fn indexes_visible_rounds_before_limiting_requests() {
    let mut fixture = Fixture::new().await;
    let record = |turn| Request {
        id: RequestId::new().to_string(),
        turn,
        // Current membership, not historical metadata, owns this projection.
        turn_index: Some(999),
        model: "fixture".into(),
        context_limit: None,
        timestamp_ms: 1,
        boundary: None,
        attempts: vec![attempt(1, Outcome::Succeeded)],
    };
    let first = fixture.queued().await;
    complete(&mut fixture, first, [observed(&record(first)), response()]).await;
    let source = fixture.session;
    let other = fixture.fork(source, first).await;
    fixture.session = other.id;
    let unrelated = fixture.queued().await;
    complete(&mut fixture, unrelated, [response()]).await;
    fixture.session = source;
    let second = fixture.queued().await;
    let mut recovered = record(second);
    recovered.attempts = vec![attempt(1, Outcome::Failed), attempt(2, Outcome::Succeeded)];
    complete(
        &mut fixture,
        second,
        [observed(&record(second)), observed(&recovered), response()],
    )
    .await;
    let expected = super::super::super::read(&fixture.database.connection, source)
        .await
        .unwrap();
    assert_eq!(expected.turns, 2);
    assert_eq!(expected.observations.calls, 3);
    assert_eq!(
        expected
            .observations
            .recent
            .iter()
            .map(|value| value.turn_index)
            .collect::<Vec<_>>(),
        [Some(1), Some(2), Some(2)]
    );
    let fork = fixture.fork(source, second).await;
    history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    for session in [source, fork.id] {
        assert_eq!(
            super::super::super::read(&fixture.database.connection, session)
                .await
                .unwrap(),
            expected
        );
    }
    let third = fixture.queued().await;
    let events = (0..RECENT_LIMIT + 2)
        .map(|_| observed(&record(third)))
        .chain([response()]);
    complete(&mut fixture, third, events).await;
    let maintenance = fixture.queued().await;
    fixture
        .database
        .connection
        .execute(
            "UPDATE turns SET kind='compaction' WHERE id=?1",
            [maintenance.to_string()],
        )
        .await
        .unwrap();
    complete(
        &mut fixture,
        maintenance,
        [observed(&record(maintenance)), response()],
    )
    .await;
    // An admitted task without history is not a recorded conversation round.
    let _pending = fixture.queued().await;
    let bounded = super::super::super::read(&fixture.database.connection, source)
        .await
        .unwrap();
    assert_eq!(bounded.turns, 3);
    assert_eq!(bounded.observations.recent.len(), RECENT_LIMIT);
    assert_eq!(bounded.observations.recent.last().unwrap().turn_index, None);
    assert!(
        bounded.observations.recent[..RECENT_LIMIT - 1]
            .iter()
            .all(|value| value.turn == third && value.turn_index == Some(3))
    );
}
