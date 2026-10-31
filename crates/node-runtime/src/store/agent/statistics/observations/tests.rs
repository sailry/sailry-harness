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
    let (_, phased, _) = advance(
        &fixture.database.connection,
        fixture.session,
        0,
        State::default(),
    )
    .await
    .unwrap();
    assert_eq!(phased.len(), expected);
    let mut rows = fixture
        .database
        .connection
        .query(
            format!("EXPLAIN QUERY PLAN {EVENTS}"),
            params![fixture.session.to_string(), 0],
        )
        .await
        .unwrap();
    let mut details = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        details.push(row.get::<String>(3).unwrap());
    }
    assert!(
        details
            .iter()
            .any(|line| line.contains("SEARCH e USING INTEGER PRIMARY KEY")),
        "{details:?} for {events} events"
    );
    assert!(
        !details
            .iter()
            .any(|line| line.contains("SORTER") || line.contains("TEMP B-TREE")),
        "{details:?}"
    );
    drop(rows);
    assert_seek(&fixture.database.connection, fixture.session).await;
}

async fn assert_seek(db: &Connection, session: SessionId) {
    let root = db
        .prepare("SELECT rootpage FROM sqlite_schema WHERE type='table' AND name='agent_events'")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get::<i64>(0)
        .unwrap();
    let mut rows = db
        .query(format!("EXPLAIN {EVENTS}"), params![session.to_string(), 7])
        .await
        .unwrap();
    let mut code = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        code.push((
            row.get::<String>(1).unwrap(),
            row.get::<i64>(2).unwrap(),
            row.get::<i64>(3).unwrap(),
            row.get::<i64>(4).unwrap(),
        ));
    }
    // Turso 0.8.3 labels rowid range searches as rowid=? in EQP. Its VDBE
    // exposes the actual exclusive seek: OpenRead.p1 is the event cursor,
    // SeekGT.p3 is its key register, and Variable.p2 receives parameter p1.
    let cursors: BTreeSet<_> = code
        .iter()
        .filter(|(op, _, page, database)| op == "OpenRead" && *page == root && *database == 0)
        .map(|(_, cursor, _, _)| *cursor)
        .collect();
    assert!(!cursors.is_empty(), "event table must be opened: {code:?}");
    assert!(
        code.iter().any(|(op, cursor, _, key)| {
            op == "SeekGT"
                && cursors.contains(cursor)
                && code.iter().any(|(op, parameter, register, _)| {
                    op == "Variable" && *parameter == 2 && register == key
                })
        }),
        "event range must seek past parameter 2: {code:?}"
    );
    assert!(
        !code.iter().any(|(op, cursor, _, _)| {
            op == "Rewind" && cursors.contains(cursor)
                || matches!(op.as_str(), "SorterOpen" | "SorterSort" | "Sort")
        }),
        "event range must not scan from the start or sort: {code:?}"
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
