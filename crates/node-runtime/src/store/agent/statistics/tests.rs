use super::*;
use crate::agent::canonical;
use crate::store::agent::tests::fixture::{message, record};
use crate::store::agent::tests::{command, fixture::Fixture};

mod context;
mod maintenance;

fn event(input: i32, output: i32) -> canonical::Record {
    let mut event = message(
        "assistant",
        canonical::Role::Assistant,
        vec![canonical::BlockKind::Text {
            text: "Usage 中文 🙂".into(),
        }],
    );
    event.message_mut().unwrap().usage = Some(canonical::Usage {
        input: u64::try_from(input).ok(),
        output: u64::try_from(output).ok(),
        cache_read: Some(2),
        reasoning: Some(1),
        ..Default::default()
    });
    event
}

fn timed(input: i32, output: i32, first_token_us: u64) -> canonical::Record {
    let mut response = event(input, output);
    response
        .metadata
        .execution
        .get_or_insert_with(Default::default)
        .timing = Some(canonical::Timing {
        first_token_us: Some(first_token_us),
        ..Default::default()
    });
    response
}

fn summary(anchor: &canonical::Record, text: &str) -> canonical::Record {
    record(
        "system",
        canonical::Kind::Compaction(canonical::Compaction {
            boundary: anchor.id.clone(),
            start: anchor.created,
            end: anchor.created,
            files: Vec::new(),
            summary: canonical::Message {
                role: canonical::Role::User,
                part_count: 1,
                content: vec![canonical::Block {
                    index: 0,
                    kind: canonical::BlockKind::Text { text: text.into() },
                }],
                ..Default::default()
            },
        }),
    )
}

#[tokio::test]
async fn follows_latest_recorded_first_text() {
    let mut fixture = Fixture::new().await;
    let read_time = async |fixture: &Fixture| {
        read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .first_token_us
    };
    fixture.append([timed(100, 20, 125_000)]).await;
    assert_eq!(read_time(&fixture).await, Some(125_000));
    let mut media = timed(500, 40, 1_000);
    media
        .metadata
        .execution
        .get_or_insert_with(Default::default)
        .media = Some(sailry_protocol::media::Kind::Image);
    fixture.append([media]).await;
    assert_eq!(read_time(&fixture).await, Some(125_000));
    fixture.append([event(200, 30)]).await;
    assert_eq!(
        read_time(&fixture).await,
        None,
        "missing timing resets stale latency"
    );
    fixture.append([timed(40, 8, 0)]).await;
    assert_eq!(read_time(&fixture).await, Some(0));
    fixture.completed(&["Unmetered response"]).await;
    assert_eq!(read_time(&fixture).await, None);
    for invalid in [
        serde_json::json!(-1),
        serde_json::json!(1.5),
        serde_json::json!("125000"),
    ] {
        let response = event(50, 10);
        assert!(
            serde_json::from_value::<canonical::Timing>(
                serde_json::json!({"first_token_us":invalid})
            )
            .is_err()
        );
        fixture.append([response]).await;
        assert_eq!(read_time(&fixture).await, None);
    }
}

#[tokio::test]
async fn counts_reported_usage_once() {
    let mut fixture = Fixture::new().await;
    assert_eq!(
        read(&fixture.database.connection, fixture.session)
            .await
            .unwrap(),
        Statistics::default()
    );
    fixture.queued().await;
    assert_eq!(
        read(&fixture.database.connection, fixture.session)
            .await
            .unwrap(),
        Statistics::default()
    );
    fixture.completed(&["No usage reported"]).await;
    assert_eq!(
        read(&fixture.database.connection, fixture.session)
            .await
            .unwrap(),
        Statistics {
            turns: 1,
            responses: 0,
            usage: None,
            observations: sailry_protocol::conversation::observation::Summary {
                replies: 1,
                ..Default::default()
            },
            ..Default::default()
        }
    );
    let first = event(i32::MAX, 4);
    fixture
        .append([first.clone(), first, event(i32::MAX, 4)])
        .await;
    fixture.append([event(-1, -5)]).await;
    assert_eq!(
        read(&fixture.database.connection, fixture.session)
            .await
            .unwrap(),
        Statistics {
            turns: 3,
            responses: 2,
            observations: sailry_protocol::conversation::observation::Summary {
                replies: 3,
                cache_read: Some(6),
                reasoning: Some(3),
                ..Default::default()
            },
            usage: Some(Usage {
                input: 2 * i32::MAX as u64,
                output: 8,
                cached_input: 4,
                reasoning: 2
            }),
            ..Default::default()
        }
    );
    assert_eq!(
        read(&fixture.database.connection, SessionId::new())
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn follows_visible_history() {
    let mut fixture = Fixture::new().await;
    let source = fixture.session;
    let first = fixture.append([event(10, 4)]).await;
    let mut last = first;
    for _ in 0..24 {
        last = fixture.append([event(10, 4)]).await;
    }
    let totals = read(&fixture.database.connection, source).await.unwrap();
    assert_eq!(totals.turns, 25);
    assert_eq!(totals.responses, 25);
    assert_eq!(totals.usage.as_ref().unwrap().input, 250);
    assert_eq!(totals.context_tokens, Some(14));
    let page = fixture.read(None, 1).await;
    assert_eq!(page.page.entries.len(), 1);
    fixture.read(page.page.next_before, 2).await;
    assert_eq!(
        read(&fixture.database.connection, source).await.unwrap(),
        totals
    );
    let Output::Session(fork) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: source,
            through: first,
            expected_revision: 1,
        },
    )
    .await
    else {
        panic!("fork expected");
    };
    assert_eq!(
        read(&fixture.database.connection, fork.id)
            .await
            .unwrap()
            .responses,
        1
    );
    fixture.session = fork.id;
    fixture.append([event(20, 8)]).await;
    let fork_totals = read(&fixture.database.connection, fork.id).await.unwrap();
    assert_eq!(fork_totals.responses, 2);
    assert_eq!(fork_totals.usage.as_ref().unwrap().input, 30);
    assert_eq!(fork_totals.context_tokens, Some(28));
    assert_eq!(
        read(&fixture.database.connection, source).await.unwrap(),
        totals
    );
    let retained = fixture.fork(source, last).await;
    let Output::Rewound(rewind) = command(
        &mut fixture.database,
        &fixture.events,
        Command::RewindConversation {
            session: source,
            through: Some(first),
            expected_head: last,
            expected_revision: 1,
        },
    )
    .await
    else {
        panic!("rewind expected");
    };
    assert_eq!(
        read(&fixture.database.connection, source)
            .await
            .unwrap()
            .responses,
        1
    );
    assert!(rewind.backup.is_none());
    assert_eq!(
        read(&fixture.database.connection, retained.id)
            .await
            .unwrap(),
        totals
    );
    assert_eq!(
        read(&fixture.database.connection, fork.id).await.unwrap(),
        fork_totals
    );
    let index = fixture
        .database
        .connection
        .prepare("SELECT sql FROM sqlite_schema WHERE type='index' AND name='agent_usage' AND tbl_name='agent_events'")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get::<String>(0)
        .unwrap();
    let transaction = turso::transaction::Transaction::new_unchecked(
        &fixture.database.connection,
        turso::transaction::TransactionBehavior::Deferred,
    )
    .await
    .unwrap();
    transaction
        .execute("DROP INDEX agent_usage", ())
        .await
        .unwrap();
    transaction.execute_batch(index).await.unwrap();
    transaction.commit().await.unwrap();
    super::super::history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    assert_eq!(
        read(&fixture.database.connection, retained.id)
            .await
            .unwrap(),
        totals
    );
}

#[tokio::test]
async fn invalidates_stale_context() {
    let mut fixture = Fixture::new().await;
    let original = timed(100, 20, 125_000);
    fixture.append([original.clone()]).await;
    let read_context = async |fixture: &Fixture| {
        let tokens = read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .context_tokens;
        assert_eq!(
            context_usage(&fixture.database.connection, fixture.session)
                .await
                .unwrap()
                .filter(|usage| usage.observed)
                .map(|usage| usage.tokens),
            tokens
        );
        tokens
    };
    assert_eq!(read_context(&fixture).await, Some(120));
    assert_eq!(
        read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .first_token_us,
        Some(125_000)
    );
    let mut media = event(500, 40);
    media
        .metadata
        .execution
        .get_or_insert_with(Default::default)
        .media = Some(sailry_protocol::media::Kind::Image);
    fixture.append([media]).await;
    assert_eq!(read_context(&fixture).await, Some(120));
    fixture.completed(&["Unmetered response"]).await;
    assert_eq!(read_context(&fixture).await, None);
    assert_eq!(
        read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .first_token_us,
        None
    );
    fixture.append([timed(200, 30, 250_000)]).await;
    assert_eq!(read_context(&fixture).await, Some(230));
    let mut summary = summary(&original, "Summary");
    let canonical::Kind::Compaction(compaction) = &mut summary.kind else {
        unreachable!()
    };
    compaction.summary.usage = event(400, 50).message_mut().unwrap().usage.take();
    summary.metadata.execution = timed(400, 50, 1_000).metadata.execution;
    fixture.append([summary]).await;
    assert_eq!(read_context(&fixture).await, None);
    assert_eq!(
        read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .first_token_us,
        None,
        "summary timing is not resumed context timing"
    );
    fixture.append([event(40, 8)]).await;
    assert_eq!(read_context(&fixture).await, Some(48));
}

#[tokio::test]
async fn restores_unique_usage() {
    let mut fixture = Fixture::new().await;
    let mut response = event(10, 4);
    response.message_mut().unwrap().usage.as_mut().unwrap().cost = Some(0.0123);
    response
        .metadata
        .execution
        .get_or_insert_with(Default::default)
        .timing = Some(canonical::Timing {
        elapsed_us: Some(2_000_000),
        ..Default::default()
    });
    let first = fixture.append([response.clone(), response]).await;
    fixture
        .database
        .connection
        .execute(
            "UPDATE agent_runs SET provider=?1",
            [serde_json::to_vec(
                &serde_json::json!({"api":"responses","authentication":"chat_gpt"}),
            )
            .unwrap()],
        )
        .await
        .unwrap();
    fixture
        .database
        .connection
        .execute(
            "INSERT INTO model_catalog(provider,id,body) VALUES('openai','fixture',?1)",
            [serde_json::to_vec(
                &serde_json::json!({"cost":{"input":2,"output":8,"cache_read":0.5}}),
            )
            .unwrap()],
        )
        .await
        .unwrap();
    let expected = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(
        expected.cost,
        Some(sailry_protocol::usage::Cost {
            usd_micros: 49,
            responses: 1,
            breakdown: Some(sailry_protocol::usage::CostBreakdown {
                input: 16,
                output: 32,
                cache_read: 1,
                cache_write: 0,
            }),
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
    fixture.append([event(10, 4)]).await;
    let partial = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
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
    )
    .await
    else {
        panic!("fork expected");
    };
    assert_eq!(
        read(&fixture.database.connection, fork.id).await.unwrap(),
        expected
    );
}

#[tokio::test]
async fn reads_indexed_totals() {
    let mut fixture = Fixture::new().await;
    for _ in 0..8 {
        let mut entry = event(10, 4);
        entry.message_mut().unwrap().content[0].kind = canonical::BlockKind::Text {
            text: "x".repeat(1024 * 1024),
        };
        fixture.append([entry]).await;
    }
    let db = &fixture.database.connection;
    let mut plan = db
        .prepare(&format!("EXPLAIN QUERY PLAN {}", report::SESSION_ROWS))
        .await
        .unwrap();
    let mut rows = plan.query([fixture.session.to_string()]).await.unwrap();
    let mut details = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        details.push(row.get::<String>(3).unwrap());
    }
    drop(rows);
    drop(plan);
    assert!(
        details.iter().any(|line| line.contains("agent_usage")),
        "{details:?}"
    );
    let totals = read(db, fixture.session).await.unwrap();
    assert_eq!(totals.responses, 8);
    assert_eq!(totals.usage.as_ref().unwrap().input, 80);
    let mut own = serde_json::to_value(&totals).unwrap();
    own.as_object_mut().unwrap().remove("delegated");
    // Observation provenance and policy add fixed metadata, never conversation bodies.
    let encoded = serde_json::to_string(&own).unwrap();
    assert!(encoded.len() < 1024);
    assert!(!encoded.contains(&"x".repeat(100)));
}
