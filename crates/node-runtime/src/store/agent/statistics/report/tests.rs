use super::*;
use crate::store::agent::tests::{command, fixture::Fixture};

mod labels;
mod location;
mod offsets;
mod requests;

pub(super) const START: i64 = 1_609_459_200_000;

pub(super) fn query() -> Query {
    Query {
        start_ms: START,
        end_ms: START + 3 * DAY,
        dimension: Dimension::Model,
        utc_offset_minutes: 0,
        projects: vec![],
        worktrees: vec![],
        providers: vec![],
        models: vec![],
        before: None,
    }
}

pub(super) fn event(timestamp_ms: i64, input: i32) -> AdkEvent {
    let mut event = AdkEvent::new("report-fixture");
    event.author = "assistant".into();
    event.timestamp = DateTime::from_timestamp_millis(timestamp_ms).unwrap();
    event.set_content(adk_core::Content::new("model").with_text("Report 中文 🙂"));
    event.llm_response.usage_metadata = Some(adk_core::UsageMetadata {
        prompt_token_count: input,
        candidates_token_count: 4,
        cache_read_input_token_count: Some(2),
        thinking_token_count: Some(1),
        ..Default::default()
    });
    event
}

async fn report(fixture: &Fixture, query: &Query) -> Report {
    read(&fixture.database.connection, fixture.database.node, query)
        .await
        .unwrap()
}

async fn session(fixture: &mut Fixture) -> Session {
    let Output::Snapshot(snapshot) =
        command(&mut fixture.database, &fixture.events, Command::Snapshot).await
    else {
        panic!("snapshot expected");
    };
    snapshot
        .sessions
        .into_iter()
        .find(|session| session.id == fixture.session)
        .unwrap()
}

#[tokio::test]
async fn estimates_custom_endpoints() {
    use serde_json::json;
    let mut fixture = Fixture::new().await;
    let session = session(&mut fixture).await;
    command(
        &mut fixture.database,
        &fixture.events,
        Command::PutProvider {
            provider: Provider {
                request_max_retries: None,
                stream_max_retries: None,
                preset: None,
                binding: None,
                consent: None,
                oauth: None,
                options: None,
                id: session.config.provider,
                revision: 0,
                name: "Custom fixture".into(),
                api: ModelApi::Responses,
                authentication: Authentication::ApiKey,
                endpoint: "https://proxy.invalid/v1".into(),
                enabled: true,
                models: vec![],
                default_model: String::new(),
                credential: None,
            },
            expected_revision: 0,
        },
    )
    .await;
    fixture
        .database
        .connection
        .execute(
            "INSERT INTO model_catalog(provider,id,body) VALUES('openai','fixture',?1)",
            [serde_json::to_vec(
                &json!({"id":"fixture","cost":{"input":2,"output":8,"cache_read":0.5}}),
            )
            .unwrap()],
        )
        .await
        .unwrap();
    let mut reported = event(START, 10);
    reported.llm_response.usage_metadata.as_mut().unwrap().cost = Some(0.5);
    fixture.append([reported]).await;
    fixture.append([event(START + DAY, 10)]).await;
    let priced = report(&fixture, &query()).await;
    assert_eq!(priced.totals.cost.as_ref().unwrap().usd_micros, 98);
    assert_eq!(priced.totals.cost.as_ref().unwrap().responses, 2);
    assert_eq!(
        priced.totals.cost.as_ref().unwrap().breakdown,
        Some(sailry_protocol::usage::CostBreakdown {
            input: 32,
            output: 64,
            cache_read: 2,
            cache_write: 0,
        })
    );
    assert_eq!(priced.days[1].metrics.cost.as_ref().unwrap().usd_micros, 49);
    assert_eq!(
        super::super::read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .cost,
        priced.totals.cost
    );
    for authentication in [
        Authentication::ApiKey,
        Authentication::ChatGpt,
        Authentication::Copilot,
    ] {
        fixture
            .database
            .connection
            .execute(
                "UPDATE agent_runs SET provider=CAST(json_set(provider,'$.authentication',?1) AS BLOB)",
                [serde_json::to_value(authentication).unwrap().as_str().unwrap()],
            ).await
            .unwrap();
        let estimated = report(&fixture, &query()).await;
        assert_eq!(estimated.totals.cost, priced.totals.cost);
        assert!(
            estimated
                .requests
                .items
                .iter()
                .all(|request| request.usd_micros == Some(49))
        );
        assert_eq!(
            super::super::read(&fixture.database.connection, fixture.session)
                .await
                .unwrap()
                .cost,
            priced.totals.cost
        );
    }
    fixture
        .database
        .connection
        .execute("DELETE FROM providers", ())
        .await
        .unwrap();
    fixture.append([event(START + 2 * DAY, 10)]).await;
    let partial = report(&fixture, &query()).await;
    assert_eq!(partial.totals.responses, 3);
    assert_eq!(partial.totals.cost, priced.totals.cost);
    fixture
        .database
        .connection
        .execute("DELETE FROM model_catalog", ())
        .await
        .unwrap();
    let unavailable = report(&fixture, &query()).await;
    assert_eq!(unavailable.totals.responses, 3);
    assert_eq!(unavailable.totals.cost, None);
    assert!(
        unavailable
            .requests
            .items
            .iter()
            .all(|request| request.usd_micros.is_none())
    );
    assert_eq!(
        super::super::read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .cost,
        None
    );
}

#[tokio::test]
async fn companion_costs() {
    use serde_json::json;
    let mut fixture = Fixture::new().await;
    let provider = ProviderId::new();
    let binding = json!({"provider":Provider {
        request_max_retries: None,
        stream_max_retries: None,
        preset: None,
        binding: None,
        consent: None,
        oauth: None,
        options: None,
        id: provider,
        revision: 1,
        name: "Frozen companion".into(),
        api: ModelApi::ChatCompletions,
        authentication: Authentication::ApiKey,
        endpoint: "https://api.openai.com/v1".into(),
        enabled: true,
        models: vec![],
        default_model: "companion".into(),
        credential: None,
    },"model":"companion"});
    fixture
        .database
        .connection
        .execute(
            "UPDATE session_media SET body=?1 WHERE session=?2",
            params![
                serde_json::to_vec(&json!({"vision":binding,"image":binding})).unwrap(),
                fixture.session.to_string()
            ],
        )
        .await
        .unwrap();
    fixture
        .database
        .connection
        .execute(
            "INSERT INTO model_catalog(provider,id,body) VALUES('openai','companion',?1)",
            [
                serde_json::to_vec(&json!({"cost":{"input":2,"output":8,"cache_read":0.5}}))
                    .unwrap(),
            ],
        )
        .await
        .unwrap();
    for kind in ["vision", "image"] {
        let mut entry = event(START, 10);
        entry.llm_response.usage_metadata.as_mut().unwrap().cost = Some(0.5);
        entry
            .provider_metadata
            .insert("sailry_media".into(), kind.into());
        fixture.append([entry]).await;
    }
    let mut selection = query();
    selection.providers = vec![provider];
    selection.models = vec!["companion".into()];
    let usage = report(&fixture, &selection).await;
    assert_eq!(usage.totals.responses, 2);
    assert_eq!(usage.totals.tokens.unwrap().input, 20);
    assert_eq!(
        usage.totals.cost.unwrap(),
        sailry_protocol::usage::Cost {
            usd_micros: 49,
            responses: 1,
            breakdown: Some(sailry_protocol::usage::CostBreakdown {
                input: 16,
                output: 32,
                cache_read: 1,
                cache_write: 0,
            }),
        }
    );
}

#[tokio::test]
async fn attributes_follow_usage_to_sent_revisions() {
    let mut fixture = Fixture::new().await;
    let source = session(&mut fixture).await;
    let mut first = event(START, 10);
    first
        .provider_metadata
        .insert("sailry_media".into(), "image".into());
    fixture.append([first]).await;
    let mut config = source.config.clone();
    config.provider = ProviderId::new();
    config.model = "revised-model".into();
    command(
        &mut fixture.database,
        &fixture.events,
        Command::SetSessionConfig {
            session: source.id,
            expected_revision: source.revision,
            config: config.clone(),
        },
    )
    .await;
    let mut second = event(START + DAY, 20);
    second
        .provider_metadata
        .insert("sailry_media".into(), "vision".into());
    fixture.append([second]).await;
    for (config, input) in [(&source.config, 10), (&config, 20)] {
        let mut selected = query();
        selected.providers = vec![config.provider];
        selected.models = vec![config.model.clone()];
        let usage = report(&fixture, &selected).await;
        assert_eq!(usage.totals.responses, 1);
        assert_eq!(usage.totals.tokens.unwrap().input, input);
        assert_eq!(usage.requests.items.len(), 1);
    }
    let overall = report(&fixture, &query()).await;
    assert_eq!(overall.totals.responses, 2);
    assert_eq!(overall.totals.tokens.unwrap().input, 30);
    assert_eq!(
        super::super::read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .responses,
        2
    );
}

#[tokio::test]
async fn preserves_branch_ownership() {
    let mut fixture = Fixture::new().await;
    let source = session(&mut fixture).await;
    let entry = event(START, 10);
    let first = fixture.append([entry.clone(), entry]).await;
    let mut changed = source.config.clone();
    changed.provider = ProviderId::new();
    changed.model = "changed-model".into();
    command(
        &mut fixture.database,
        &fixture.events,
        Command::SetSessionConfig {
            session: source.id,
            expected_revision: 1,
            config: changed.clone(),
        },
    )
    .await;
    let Output::Session(fork) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: source.id,
            through: first,
            expected_revision: 2,
        },
    )
    .await
    else {
        panic!("fork expected");
    };
    fixture.session = fork.id;
    fixture.append([event(START + DAY, 20)]).await;
    let before = report(&fixture, &query()).await;
    assert_eq!(before.totals.responses, 2);
    assert_eq!(before.totals.tokens.as_ref().unwrap().input, 30);
    assert_eq!(before.groups.len(), 2);
    assert_eq!(before.days[0].models[&source.config.model].input, 10);
    assert_eq!(before.days[1].models[&changed.model].input, 20);
    assert!(before.days[2].models.is_empty());
    assert_eq!(
        before
            .recent
            .iter()
            .flat_map(|point| point.models.values())
            .map(|tokens| tokens.input)
            .sum::<u64>(),
        20
    );
    assert_eq!(before.resources.len(), 6);
    for key in [
        Key::Provider(source.config.provider),
        Key::Provider(changed.provider),
        Key::Project(source.project.unwrap()),
        Key::Worktree {
            project: source.project.unwrap(),
            worktree: source.worktree,
        },
        Key::Model {
            provider: source.config.provider,
            model: source.config.model.clone(),
        },
        Key::Model {
            provider: changed.provider,
            model: changed.model.clone(),
        },
    ] {
        assert!(before.resources.contains(&key));
    }
    assert!(before.groups.iter().any(|group| group.key
        == Key::Model {
            provider: source.config.provider,
            model: source.config.model.clone()
        }
        && group.metrics.tokens.as_ref().unwrap().input == 10));
    command(
        &mut fixture.database,
        &fixture.events,
        Command::RewindConversation {
            session: source.id,
            through: None,
            expected_head: first,
            expected_revision: 1,
        },
    )
    .await;
    let after = report(&fixture, &query()).await;
    assert!(after.cursor > before.cursor);
    assert_eq!(after.totals, before.totals);
    assert_eq!(after.groups, before.groups);
    assert_eq!(after.resources, before.resources);
    let mut filtered = query();
    filtered.projects = vec![source.project.unwrap()];
    filtered.worktrees = vec![source.worktree];
    filtered.providers = vec![changed.provider];
    filtered.models = vec![changed.model];
    assert_eq!(
        report(&fixture, &filtered)
            .await
            .totals
            .tokens
            .unwrap()
            .input,
        20
    );
    filtered.models = vec![source.config.model.clone()];
    let empty = report(&fixture, &filtered).await;
    assert_eq!(empty.totals, Metrics::default());
    assert!(empty.resources.is_empty());
    assert!(
        empty
            .days
            .iter()
            .chain(&empty.recent)
            .all(|point| point.models.is_empty())
    );
    for (dimension, key) in [
        (Dimension::Provider, Key::Provider(source.config.provider)),
        (Dimension::Project, Key::Project(source.project.unwrap())),
        (
            Dimension::Worktree,
            Key::Worktree {
                project: source.project.unwrap(),
                worktree: source.worktree,
            },
        ),
    ] {
        let mut query = query();
        query.dimension = dimension;
        query.providers = vec![source.config.provider];
        let filtered = report(&fixture, &query).await;
        assert!(filtered.days.iter().chain(&filtered.recent).all(|point| {
            point
                .models
                .keys()
                .all(|model| model == &source.config.model)
        }));
        assert_eq!(filtered.resources.len(), 4);
        assert!(filtered.resources.contains(&Key::Model {
            provider: source.config.provider,
            model: source.config.model.clone()
        }));
        assert!(
            !filtered
                .resources
                .contains(&Key::Provider(changed.provider))
        );
        let groups = filtered.groups;
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key, key);
        assert_eq!(groups[0].metrics.tokens.as_ref().unwrap().input, 10);
    }
}

#[tokio::test]
async fn preserves_bucket_boundaries() {
    let mut fixture = Fixture::new().await;
    assert_eq!(report(&fixture, &query()).await.totals, Metrics::default());
    fixture.completed(&["No reported usage"]).await;
    let mut precise = event(START + 999, i32::MAX);
    precise.timestamp += chrono::Duration::microseconds(999);
    fixture
        .append([
            event(START - 1, 1),
            event(START, 10),
            precise,
            event(START + DAY, 20),
            event(START + 3 * DAY, 1),
        ])
        .await;
    let report = report(&fixture, &query()).await;
    assert_eq!(report.totals.responses, 3);
    assert_eq!(
        report.totals.tokens.as_ref().unwrap().input,
        i32::MAX as u64 + 30
    );
    assert_eq!(report.days.len(), 3);
    assert_eq!(report.days[0].metrics.responses, 2);
    assert_eq!(report.days[1].metrics.responses, 1);
    assert_eq!(report.days[2].metrics, Metrics::default());
    assert_eq!(report.recent.len(), 96);
    assert_eq!(report.recent[0].metrics.responses, 1);
    assert_eq!(
        report
            .recent
            .iter()
            .map(|point| point.metrics.responses)
            .sum::<u64>(),
        1
    );
    let mut query = query();
    query.start_ms += 500;
    query.end_ms = START + 1000;
    assert_eq!(
        read(&fixture.database.connection, fixture.database.node, &query)
            .await
            .unwrap()
            .totals
            .responses,
        1
    );
}

#[tokio::test]
async fn rejects_oversized_queries() {
    let mut fixture = Fixture::new().await;
    for (start, end) in [
        (-1, 1),
        (0, 0),
        (1, 0),
        (0, 367 * DAY),
        (i64::MAX - DAY, i64::MAX),
    ] {
        let mut query = query();
        query.start_ms = start;
        query.end_ms = end;
        assert_eq!(
            read(&fixture.database.connection, fixture.database.node, &query)
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    let mut filters = query();
    filters.models = vec!["x".into(); 129];
    assert!(
        read(
            &fixture.database.connection,
            fixture.database.node,
            &filters
        )
        .await
        .is_err()
    );
    fixture.append([event(START, 1)]).await;
    let transaction = fixture
        .database
        .connection
        .transaction_with_behavior(turso::transaction::TransactionBehavior::Concurrent)
        .await
        .unwrap();
    let first = crate::store::database::counters::next(
        &transaction,
        crate::store::database::counters::Namespace::AgentEvents,
    )
    .await
    .unwrap();
    let indices = serde_json::to_string(&(0..MAX_ROWS).collect::<Vec<_>>()).unwrap();
    transaction
        .execute(
            "INSERT INTO agent_events(sequence,session,id,turn,body)
         SELECT ?1+n.value,e.session,'report-limit-'||n.value,e.turn,
           CAST(json_set(e.body,'$.id','report-limit-'||n.value) AS BLOB)
         FROM (SELECT * FROM agent_events LIMIT 1) e,json_each(?2) n",
            params![first as i64, indices],
        )
        .await
        .unwrap();
    // This bulk fixture reserves its entire explicit range in the same transaction.
    transaction
        .execute(
            "UPDATE counters SET value=?1 WHERE namespace='agent_events'",
            [i64::try_from(first + MAX_ROWS as u64 - 1).unwrap()],
        )
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    let error = read(
        &fixture.database.connection,
        fixture.database.node,
        &query(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(error.message.contains("row limit"));
}

#[tokio::test]
async fn reads_indexed_metadata() {
    let mut fixture = Fixture::new().await;
    for _ in 0..8 {
        let mut event = event(START, 10);
        event.set_content(adk_core::Content::new("model").with_text("x".repeat(1024 * 1024)));
        fixture.append([event]).await;
    }
    let db = &fixture.database.connection;
    let mut plan = db
        .prepare(&format!("EXPLAIN QUERY PLAN {}", query::ROWS))
        .await
        .unwrap();
    let mut rows = plan
        .query(params![
            "2021-01-01T00:00:00",
            "2021-01-04T00:00:00",
            "[]",
            "[]",
            "[]",
            "[]",
            MAX_ROWS as i64 + 1
        ])
        .await
        .unwrap();
    let mut details = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        details.push(row.get::<String>(3).unwrap());
    }
    drop(rows);
    drop(plan);
    assert!(
        details.iter().any(|line| line.contains("agent_usage_time")),
        "{details:?}"
    );
    let report = report(&fixture, &query()).await;
    assert_eq!(report.totals.responses, 8);
    let encoded = serde_json::to_vec(&report).unwrap();
    // Resource labels and latency are part of the report, model response bodies are not.
    assert!(encoded.len() < 20 * 1024);
    db.execute(
        "UPDATE agent_events SET body=json_set(body,'$.content.parts[0].text','short')",
        (),
    )
    .await
    .unwrap();
    assert_eq!(
        encoded,
        serde_json::to_vec(&read(db, fixture.database.node, &query()).await.unwrap()).unwrap()
    );
}
