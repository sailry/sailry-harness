use super::*;
use crate::store::agent::tests::{command, fixture::Fixture};

mod context;

fn event(input: i32, output: i32) -> AdkEvent {
    let mut event = AdkEvent::new("statistics-fixture");
    event.author = "assistant".into();
    event.set_content(adk_core::Content::new("model").with_text("Usage 中文 🙂"));
    event.llm_response.usage_metadata = Some(adk_core::UsageMetadata {
        prompt_token_count: input,
        candidates_token_count: output,
        cache_read_input_token_count: Some(2),
        thinking_token_count: Some(1),
        ..Default::default()
    });
    event
}

#[test]
fn counts_reported_usage_once() {
    let mut fixture = Fixture::new();
    assert_eq!(
        read(&fixture.database.connection, fixture.session).unwrap(),
        Statistics::default()
    );
    fixture.queued();
    assert_eq!(
        read(&fixture.database.connection, fixture.session).unwrap(),
        Statistics::default()
    );
    fixture.completed(&["No usage reported"]);
    assert_eq!(
        read(&fixture.database.connection, fixture.session).unwrap(),
        Statistics {
            turns: 1,
            responses: 0,
            usage: None,
            ..Default::default()
        }
    );
    let first = event(i32::MAX, 4);
    fixture.append([first.clone(), first, event(i32::MAX, 4)]);
    fixture.append([event(-1, -5)]);
    assert_eq!(
        read(&fixture.database.connection, fixture.session).unwrap(),
        Statistics {
            turns: 3,
            responses: 3,
            usage: Some(Usage {
                input: 2 * i32::MAX as u64,
                output: 8,
                cached_input: 6,
                reasoning: 3
            }),
            ..Default::default()
        }
    );
    assert_eq!(
        read(&fixture.database.connection, SessionId::new())
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}

#[test]
fn follows_visible_history() {
    let mut fixture = Fixture::new();
    let source = fixture.session;
    let first = fixture.append([event(10, 4)]);
    let mut last = first;
    for _ in 0..24 {
        last = fixture.append([event(10, 4)]);
    }
    let totals = read(&fixture.database.connection, source).unwrap();
    assert_eq!(totals.turns, 25);
    assert_eq!(totals.responses, 25);
    assert_eq!(totals.usage.as_ref().unwrap().input, 250);
    assert_eq!(totals.context_tokens, Some(14));
    let page = fixture.read(None, 1);
    assert_eq!(page.page.entries.len(), 1);
    fixture.read(page.page.next_before, 2);
    assert_eq!(read(&fixture.database.connection, source).unwrap(), totals);
    let Output::Session(fork) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: source,
            through: first,
            expected_revision: 1,
        },
    ) else {
        panic!("fork expected");
    };
    assert_eq!(
        read(&fixture.database.connection, fork.id)
            .unwrap()
            .responses,
        1
    );
    fixture.session = fork.id;
    fixture.append([event(20, 8)]);
    let fork_totals = read(&fixture.database.connection, fork.id).unwrap();
    assert_eq!(fork_totals.responses, 2);
    assert_eq!(fork_totals.usage.as_ref().unwrap().input, 30);
    assert_eq!(fork_totals.context_tokens, Some(28));
    assert_eq!(read(&fixture.database.connection, source).unwrap(), totals);
    let Output::Rewound(rewind) = command(
        &mut fixture.database,
        &fixture.events,
        Command::RewindConversation {
            session: source,
            through: Some(first),
            expected_head: last,
            expected_revision: 1,
        },
    ) else {
        panic!("rewind expected");
    };
    assert_eq!(
        read(&fixture.database.connection, source)
            .unwrap()
            .responses,
        1
    );
    assert_eq!(
        read(&fixture.database.connection, rewind.backup.id).unwrap(),
        totals
    );
    assert_eq!(
        read(&fixture.database.connection, fork.id).unwrap(),
        fork_totals
    );
    fixture
        .database
        .connection
        .execute_batch("REINDEX agent_usage")
        .unwrap();
    super::super::history::rebuild(&mut fixture.database.connection).unwrap();
    assert_eq!(
        read(&fixture.database.connection, rewind.backup.id).unwrap(),
        totals
    );
}

#[test]
fn invalidates_stale_context() {
    let mut fixture = Fixture::new();
    let original = event(100, 20);
    fixture.append([original.clone()]);
    let read_context = |fixture: &Fixture| {
        let tokens = read(&fixture.database.connection, fixture.session)
            .unwrap()
            .context_tokens;
        assert_eq!(
            context_usage(&fixture.database.connection, fixture.session)
                .unwrap()
                .map(|usage| usage.tokens),
            tokens
        );
        tokens
    };
    assert_eq!(read_context(&fixture), Some(120));
    let mut media = event(500, 40);
    media
        .provider_metadata
        .insert("sailry_media".into(), "image".into());
    fixture.append([media]);
    assert_eq!(read_context(&fixture), Some(120));
    fixture.completed(&["Unmetered response"]);
    assert_eq!(read_context(&fixture), None);
    fixture.append([event(200, 30)]);
    assert_eq!(read_context(&fixture), Some(230));
    let mut summary = AdkEvent::new("summary-fixture");
    summary.author = "system".into();
    summary
        .provider_metadata
        .insert(super::super::compaction::END_EVENT.into(), original.id);
    summary.actions.compaction = Some(adk_core::EventCompaction {
        start_timestamp: original.timestamp,
        end_timestamp: original.timestamp,
        compacted_content: adk_core::Content::new("model").with_text("Summary"),
    });
    summary.llm_response.usage_metadata = event(400, 50).llm_response.usage_metadata;
    fixture.append([summary]);
    assert_eq!(read_context(&fixture), None);
    fixture.append([event(40, 8)]);
    assert_eq!(read_context(&fixture), Some(48));
}

#[test]
fn restores_unique_usage() {
    let mut fixture = Fixture::new();
    let mut response = event(10, 4);
    response.llm_response.usage_metadata.as_mut().unwrap().cost = Some(0.0123);
    response.llm_response.provider_metadata =
        Some(serde_json::json!({"sailry_timing":{"elapsed_us":2_000_000}}));
    let first = fixture.append([response.clone(), response]);
    let expected = read(&fixture.database.connection, fixture.session).unwrap();
    assert_eq!(
        expected.cost,
        Some(sailry_protocol::usage::Cost {
            usd_micros: 12_300,
            responses: 1,
            breakdown: None,
        })
    );
    assert_eq!(
        expected.generation,
        Some(sailry_protocol::usage::Generation {
            elapsed_us: 2_000_000,
            output_tokens: 4,
            responses: 1
        })
    );
    fixture.append([event(10, 4)]);
    let partial = read(&fixture.database.connection, fixture.session).unwrap();
    assert_eq!(partial.responses, 2);
    assert_eq!(partial.cost, expected.cost);
    assert_eq!(partial.generation, expected.generation);
    let Output::Session(fork) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: fixture.session,
            through: first,
            expected_revision: 1,
        },
    ) else {
        panic!("fork expected");
    };
    assert_eq!(
        read(&fixture.database.connection, fork.id).unwrap(),
        expected
    );
}

#[test]
fn reads_indexed_totals() {
    let mut fixture = Fixture::new();
    for _ in 0..8 {
        let mut entry = event(10, 4);
        entry.set_content(adk_core::Content::new("model").with_text("x".repeat(1024 * 1024)));
        fixture.append([entry]);
    }
    let db = &fixture.database.connection;
    let mut plan = db.prepare(&format!("EXPLAIN QUERY PLAN {TOTALS}")).unwrap();
    let details = plan
        .query_map([fixture.session.to_string()], |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        details.iter().any(|line| line.contains("agent_usage")),
        "{details:?}"
    );
    let totals = read(db, fixture.session).unwrap();
    assert_eq!(totals.responses, 8);
    assert_eq!(totals.usage.as_ref().unwrap().input, 80);
    assert!(serde_json::to_vec(&totals).unwrap().len() < 256);
}
