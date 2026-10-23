use super::*;
use crate::store::agent::{history, runs, tests::fixture::Fixture};
use sailry_protocol::conversation::observation::Phases;

mod requests;

fn phase(value: Phases) -> AdkEvent {
    let mut event = filler();
    event.provider_metadata.insert(
        "sailry_execution_phases".into(),
        serde_json::to_string(&value).unwrap(),
    );
    event
}

fn filler() -> AdkEvent {
    let mut event = AdkEvent::new("observations-fixture");
    event.author = "system".into();
    event
}

fn response() -> AdkEvent {
    let mut event = AdkEvent::new("observations-fixture");
    event.author = "assistant".into();
    event.set_content(adk_core::Content::new("model").with_text("Response"));
    event.llm_response.usage_metadata = Some(adk_core::UsageMetadata {
        prompt_token_count: 10,
        candidates_token_count: 4,
        cache_read_input_token_count: Some(2),
        thinking_token_count: Some(1),
        ..Default::default()
    });
    event
}

fn complete(fixture: &mut Fixture, turn: TurnId, events: impl IntoIterator<Item = AdkEvent>) {
    runs::start(&fixture.database.connection, turn).unwrap();
    assert_eq!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .unwrap()
            .turn
            .id,
        turn
    );
    let transaction = fixture.database.connection.transaction().unwrap();
    for event in events {
        history::insert(&transaction, fixture.session, turn, event)
            .unwrap()
            .unwrap();
    }
    transaction.commit().unwrap();
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .unwrap();
}

fn assert_reads(fixture: &Fixture, events: usize, expected: usize) {
    let mut query = fixture.database.connection.prepare(PHASES).unwrap();
    let mut rows = query.query([fixture.session.to_string()]).unwrap();
    let mut count = 0;
    while rows.next().unwrap().is_some() {
        count += 1;
    }
    drop(rows);
    assert_eq!(count, expected);
    let steps = query.get_status(rusqlite::StatementStatus::VmStep);
    // Allow a full history scan per turn, including turns without phase records.
    assert!(
        steps < (events * 64) as i32,
        "{steps} steps for {events} events"
    );
}

#[test]
fn keeps_latest_phases_across_long_history() {
    const DEPTH: usize = 1024;
    let mut fixture = Fixture::new();
    let first_phase = Phases {
        active_us: Some(10),
        waiting_us: Some(20),
        output_us: Some(30),
        tools_us: Some(40),
        other_us: Some(50),
    };
    let first_response = response();
    let first_time = first_response.timestamp.timestamp_millis();
    let first = fixture.queued();
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
    );
    let last_response = response();
    let last_time = last_response.timestamp.timestamp_millis();
    let last = fixture.queued();
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
    );
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
        super::super::read(&fixture.database.connection, fixture.session).unwrap(),
        expected
    );
    assert_reads(&fixture, (DEPTH + 3) * 2, 2);
    let source = fixture.session;
    let fork = fixture.fork(source, first);
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
        super::super::read(&fixture.database.connection, fork.id).unwrap(),
        inherited
    );
    history::rebuild(&mut fixture.database.connection).unwrap();
    for (session, expected) in [(source, expected), (fork.id, inherited)] {
        assert_eq!(
            super::super::read(&fixture.database.connection, session).unwrap(),
            expected
        );
    }
}

#[test]
fn bounds_unrecorded_history() {
    const DEPTH: usize = 2048;
    let mut fixture = Fixture::new();
    let known = Phases {
        active_us: Some(10),
        waiting_us: Some(2),
        output_us: Some(3),
        tools_us: Some(4),
        other_us: Some(1),
    };
    fixture.append([phase(known.clone())]);
    let missing = fixture.queued();
    assert_eq!(
        read(&fixture.database.connection, fixture.session).unwrap(),
        Summary {
            phases: known,
            ..Default::default()
        }
    );
    complete(
        &mut fixture,
        missing,
        std::iter::repeat_with(filler).take(DEPTH),
    );
    assert_eq!(
        read(&fixture.database.connection, fixture.session).unwrap(),
        Summary::default()
    );
    assert_reads(&fixture, DEPTH + 1, 1);
}
