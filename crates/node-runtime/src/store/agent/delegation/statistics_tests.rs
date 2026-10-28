use super::fixture::{Fixture, execute, finish, invoke, setup, start};
use super::*;
use crate::agent::canonical;
use crate::store::agent::tests::fixture::message;
use crate::store::agent::{statistics, tests::command};
use sailry_protocol::usage::{Dimension, Metrics, Query};
use serde_json::json;

fn event(input: i32, elapsed: u64) -> canonical::Record {
    let mut event = message(
        "assistant",
        canonical::Role::Assistant,
        vec![canonical::BlockKind::Text {
            text: "Measured response".into(),
        }],
    );
    event.message_mut().unwrap().usage = Some(canonical::Usage {
        input: u64::try_from(input).ok(),
        output: Some(4),
        cache_read: Some(2),
        reasoning: Some(1),
        ..Default::default()
    });
    event
        .metadata
        .execution
        .get_or_insert_with(Default::default)
        .timing = Some(canonical::Timing {
        elapsed_us: Some(elapsed),
        ..Default::default()
    });
    event
}

async fn append(fixture: &mut Fixture, session: SessionId, turn: TurnId, event: canonical::Record) {
    history::append(&mut fixture.database, session, turn, event, &fixture.events)
        .await
        .unwrap();
}

async fn metrics(fixture: &Fixture, session: SessionId) -> Statistics {
    statistics::read(&fixture.database.connection, session)
        .await
        .unwrap()
}

#[tokio::test]
async fn follows_visible_delegations_without_duplication() {
    let mut fixture = setup().await;
    let parent = fixture.session;
    let first = start(&mut fixture).await;
    let child = invoke(&mut fixture, first, json!({"task":"Initial child task"}))
        .await
        .unwrap();
    append(&mut fixture, parent, first, event(10, 200)).await;
    let response = event(20, 2_000);
    append(
        &mut fixture,
        child.turn.session,
        child.turn.id,
        response.clone(),
    )
    .await;
    append(&mut fixture, child.turn.session, child.turn.id, response).await;
    let request = Request::new(
        fixture.database.node,
        Command::GuideTurn {
            recipient: child.turn.id,
            expected_revision: child.turn.revision,
            message: "Continue with guidance".into(),
        },
    );
    let Output::QueuedTurn(guidance) = execute(&mut fixture, request).await.unwrap() else {
        panic!("guided turn expected")
    };
    let inputs = runs::guidance::take(&mut fixture.database, child.turn.id, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(inputs[0].turn.id, guidance.id);
    let input = message(
        "user",
        canonical::Role::User,
        vec![canonical::BlockKind::Text {
            text: inputs[0].message.text.clone(),
        }],
    );
    append(&mut fixture, child.turn.session, guidance.id, input).await;
    append(
        &mut fixture,
        child.turn.session,
        child.turn.id,
        event(40, 4_000),
    )
    .await;
    finish(&mut fixture, child.turn.id).await;
    let consumed = super::super::background::consume_child(
        &fixture.database.connection,
        first,
        child.turn.session,
        child.turn.id,
    )
    .await
    .unwrap();
    assert_eq!(consumed.len(), 1);
    finish(&mut fixture, first).await;
    let Output::Session(fork) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: parent,
            through: first,
            expected_revision: 1,
        },
    )
    .await
    else {
        panic!("fork expected")
    };
    let first_totals = metrics(&fixture, parent).await;
    assert_eq!(metrics(&fixture, fork.id).await, first_totals);
    assert_eq!(first_totals.responses, 1);
    assert_eq!(first_totals.delegated.responses, 2);
    assert_eq!(first_totals.delegated.tokens.as_ref().unwrap().input, 60);
    let parents = statistics::parents(&fixture.database.connection, child.turn.id)
        .await
        .unwrap();
    assert_eq!(parents.len(), 2);
    assert!(parents.contains(&parent) && parents.contains(&fork.id));

    let second = start(&mut fixture).await;
    let reused = invoke(
        &mut fixture,
        second,
        json!({"session":child.turn.session,"task":"Reuse the child"}),
    )
    .await
    .unwrap();
    let sibling = invoke(
        &mut fixture,
        second,
        json!({"task":"Independent sibling task"}),
    )
    .await
    .unwrap();
    append(&mut fixture, parent, second, event(15, 300)).await;
    assert_eq!(reused.turn.session, child.turn.session);
    append(
        &mut fixture,
        reused.turn.session,
        reused.turn.id,
        event(30, 3_000),
    )
    .await;
    finish(&mut fixture, reused.turn.id).await;
    append(
        &mut fixture,
        sibling.turn.session,
        sibling.turn.id,
        event(50, 5_000),
    )
    .await;
    finish(&mut fixture, sibling.turn.id).await;
    for child in [&reused.turn, &sibling.turn] {
        let consumed = super::super::background::consume_child(
            &fixture.database.connection,
            second,
            child.session,
            child.id,
        )
        .await
        .unwrap();
        assert_eq!(consumed.len(), 1);
    }
    finish(&mut fixture, second).await;
    let db = &fixture.database.connection;
    db.execute(
        "UPDATE agent_runs SET provider=?1",
        [serde_json::to_vec(&json!({"api":"responses","authentication":"chat_gpt"})).unwrap()],
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO model_catalog(provider,id,body) VALUES('openai','fixture',?1)",
        [serde_json::to_vec(&json!({"cost":{"input":1,"output":1,"cache_read":1}})).unwrap()],
    )
    .await
    .unwrap();
    let totals = metrics(&fixture, parent).await;
    assert_eq!(totals.turns, 2);
    assert_eq!(totals.responses, 2);
    assert_eq!(totals.usage.as_ref().unwrap().input, 25);
    assert_eq!(totals.context_tokens, Some(19));
    assert_eq!(totals.generation.as_ref().unwrap().elapsed_us, 500);
    assert_eq!(totals.generation.as_ref().unwrap().output_tokens, 8);
    assert_eq!(totals.cost.as_ref().unwrap().usd_micros, 33);
    assert_eq!(totals.delegated.responses, 4);
    assert_eq!(totals.delegated.tokens.as_ref().unwrap().input, 140);
    assert_eq!(totals.delegated.tokens.as_ref().unwrap().output, 16);
    assert_eq!(totals.delegated.tokens.as_ref().unwrap().cached_input, 8);
    assert_eq!(totals.delegated.tokens.as_ref().unwrap().reasoning, 4);
    assert_eq!(
        totals.delegated.generation.as_ref().unwrap().elapsed_us,
        14_000
    );
    assert_eq!(totals.delegated.cost.as_ref().unwrap().usd_micros, 156);
    assert_eq!(
        metrics(&fixture, child.turn.session).await.delegated,
        Metrics::default()
    );
    let inherited = metrics(&fixture, fork.id).await;
    assert_eq!(inherited.responses, 1);
    assert_eq!(inherited.delegated.responses, 2);
    assert_eq!(inherited.delegated.tokens.as_ref().unwrap().input, 60);
    assert_eq!(inherited.context_tokens, Some(14));
    assert_eq!(
        statistics::parents(db, reused.turn.id).await.unwrap(),
        [parent]
    );
    let now = chrono::Utc::now().timestamp_millis();
    let query = Query {
        start_ms: now - 86_400_000,
        end_ms: now + 86_400_000,
        dimension: Dimension::Model,
        utc_offset_minutes: 0,
        projects: vec![],
        worktrees: vec![],
        providers: vec![],
        models: vec![],
        before: None,
    };
    let report = statistics::report::read(db, fixture.database.node, &query)
        .await
        .unwrap();
    assert_eq!(report.totals.responses, 6);
    assert_eq!(report.totals.tokens.as_ref().unwrap().input, 165);
    assert_eq!(report.totals.cost.as_ref().unwrap().usd_micros, 189);
    assert_eq!(report.requests.items.len(), 6);
    let Output::Session(retained) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: parent,
            through: second,
            expected_revision: 1,
        },
    )
    .await
    else {
        panic!("fork expected")
    };
    let Output::Rewound(rewound) = command(
        &mut fixture.database,
        &fixture.events,
        Command::RewindConversation {
            session: parent,
            through: Some(first),
            expected_head: second,
            expected_revision: 1,
        },
    )
    .await
    else {
        panic!("rewound conversation expected")
    };
    assert_eq!(metrics(&fixture, parent).await, inherited);
    assert!(rewound.backup.is_none());
    assert_eq!(metrics(&fixture, retained.id).await, totals);
    let after =
        statistics::report::read(&fixture.database.connection, fixture.database.node, &query)
            .await
            .unwrap();
    assert_eq!(after.totals, report.totals);
}
