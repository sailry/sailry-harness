use super::*;
use crate::agent::canonical;
use crate::store::agent::tests::fixture::{message, record};
use crate::store::agent::{history, runs, tests::fixture::Fixture};
use sailry_protocol::conversation::observation::Phases;

mod requests;

fn phase(value: Phases) -> canonical::Record {
    let mut event = filler();
    event
        .metadata
        .execution
        .get_or_insert_with(Default::default)
        .phases = Some(value);
    event
}

fn filler() -> canonical::Record {
    let mut event = record(
        "system",
        canonical::Kind::Message(canonical::Message::default()),
    );
    event.metadata.runtime = Some(canonical::Runtime {
        user_visible: false,
        agent_visible: false,
        ..Default::default()
    });
    event
}

fn response() -> canonical::Record {
    let mut event = message(
        "assistant",
        canonical::Role::Assistant,
        vec![canonical::BlockKind::Text {
            text: "Response".into(),
        }],
    );
    event.message_mut().unwrap().usage = Some(canonical::Usage {
        input: Some(10),
        output: Some(4),
        cache_read: Some(2),
        reasoning: Some(1),
        ..Default::default()
    });
    event
}

async fn complete(
    fixture: &mut Fixture,
    turn: TurnId,
    events: impl IntoIterator<Item = canonical::Record>,
) {
    runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    assert_eq!(
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .unwrap()
            .turn
            .id,
        turn
    );
    let transaction = fixture
        .database
        .connection
        .transaction_with_behavior(turso::transaction::TransactionBehavior::Concurrent)
        .await
        .unwrap();
    for mut event in events {
        event.turn = turn;
        history::insert(&transaction, fixture.session, turn, event)
            .await
            .unwrap()
            .unwrap();
    }
    transaction.commit().await.unwrap();
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
}

async fn assert_reads(fixture: &Fixture, events: usize, expected: usize) {
    let mut query = fixture.database.connection.prepare(PHASES).await.unwrap();
    let mut rows = query.query([fixture.session.to_string()]).await.unwrap();
    let mut count = 0;
    while rows.next().await.unwrap().is_some() {
        count += 1;
    }
    drop(rows);
    drop(query);
    assert_eq!(count, expected);
    let mut rows = fixture
        .database
        .connection
        .query(
            format!("EXPLAIN QUERY PLAN {PHASES}"),
            [fixture.session.to_string()],
        )
        .await
        .unwrap();
    let mut details = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        details.push(row.get::<String>(3).unwrap());
    }
    // The native SDK exposes plans, not executed VM steps. Require one indexed
    // history lookup per turn, including turns without phase records.
    assert!(
        details.iter().any(|line| line.starts_with("SEARCH latest ")
            && line.contains("agent_events_turn")
            && line.contains("turn=?")),
        "{details:?} for {events} events"
    );
    assert!(
        !details.iter().any(|line| line.starts_with("SCAN ")
            && (line.contains("latest") || line.contains("agent_events"))),
        "{details:?} for {events} events"
    );
}

#[tokio::test]
async fn keeps_latest_phases_across_long_history() {
    const DEPTH: usize = 1024;
    let mut fixture = Fixture::new().await;
    let first_phase = Phases {
        active_us: Some(10),
        waiting_us: Some(20),
        output_us: Some(30),
        tools_us: Some(40),
        other_us: Some(50),
    };
    let first_response = response();
    let first_time = first_response.created.timestamp_millis();
    let first = fixture.queued().await;
    complete(
        &mut fixture,
        first,
        [
            first_response,
            phase(Phases::default()),
            phase(first_phase.clone()),
        ]
        .into_iter()
        .chain(std::iter::repeat_with(filler).take(DEPTH)),
    )
    .await;
    let last_response = response();
    let last_time = last_response.created.timestamp_millis();
    let last = fixture.queued().await;
    complete(
        &mut fixture,
        last,
        [
            last_response,
            phase(first_phase.clone()),
            phase(Phases {
                active_us: Some(1),
                waiting_us: None,
                output_us: Some(3),
                tools_us: Some(4),
                other_us: Some(u64::MAX),
            }),
        ]
        .into_iter()
        .chain(std::iter::repeat_with(filler).take(DEPTH)),
    )
    .await;
    let expected = Statistics {
        turns: 2,
        responses: 2,
        usage: Some(Usage {
            input: 20,
            output: 8,
            cached_input: 4,
            reasoning: 2,
        }),
        context_tokens: Some(14),
        context_model: Some("fixture".into()),
        context_timestamp_ms: Some(last_time),
        observations: Summary {
            replies: 2,
            cache_read: Some(4),
            reasoning: Some(2),
            phases: Phases {
                active_us: Some(11),
                waiting_us: None,
                output_us: Some(33),
                tools_us: Some(44),
                other_us: Some(u64::MAX),
            },
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(
        super::super::read(&fixture.database.connection, fixture.session)
            .await
            .unwrap(),
        expected
    );
    assert_reads(&fixture, (DEPTH + 3) * 2, 2).await;
    let source = fixture.session;
    let fork = fixture.fork(source, first).await;
    let inherited = Statistics {
        turns: 1,
        responses: 1,
        usage: Some(Usage {
            input: 10,
            output: 4,
            cached_input: 2,
            reasoning: 1,
        }),
        context_tokens: Some(14),
        context_model: Some("fixture".into()),
        context_timestamp_ms: Some(first_time),
        observations: Summary {
            replies: 1,
            cache_read: Some(2),
            reasoning: Some(1),
            phases: first_phase,
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(
        super::super::read(&fixture.database.connection, fork.id)
            .await
            .unwrap(),
        inherited
    );
    history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    for (session, expected) in [(source, expected), (fork.id, inherited)] {
        assert_eq!(
            super::super::read(&fixture.database.connection, session)
                .await
                .unwrap(),
            expected
        );
    }
}

#[tokio::test]
async fn bounds_unrecorded_history() {
    const DEPTH: usize = 2048;
    let mut fixture = Fixture::new().await;
    let known = Phases {
        active_us: Some(10),
        waiting_us: Some(2),
        output_us: Some(3),
        tools_us: Some(4),
        other_us: Some(1),
    };
    fixture.append([phase(known.clone())]).await;
    let missing = fixture.queued().await;
    assert_eq!(
        read(&fixture.database.connection, fixture.session)
            .await
            .unwrap(),
        Summary {
            phases: known,
            ..Default::default()
        }
    );
    complete(
        &mut fixture,
        missing,
        std::iter::repeat_with(filler).take(DEPTH),
    )
    .await;
    assert_eq!(
        read(&fixture.database.connection, fixture.session)
            .await
            .unwrap(),
        Summary::default()
    );
    assert_reads(&fixture, DEPTH + 1, 1).await;
}
