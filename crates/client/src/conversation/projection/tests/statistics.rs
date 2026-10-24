use super::*;
use sailry_protocol::conversation::{Statistics, Usage};

#[test]
fn recovers_totals_across_generations() {
    let (mut projection, mut snapshot, _, _) = fixture();
    snapshot.statistics = Statistics {
        turns: 25,
        responses: 30,
        context_tokens: Some(16),
        context_limit: Some(1_024),
        first_token_us: Some(125_000),
        usage: Some(Usage {
            input: 300,
            output: 60,
            cached_input: 20,
            reasoning: 10,
        }),
        ..Default::default()
    };
    snapshot.statistics.observations.calls = 7;
    snapshot.statistics.observations.phases.active_us = Some(5_000);
    snapshot.statistics.context_model = Some("observed-model".into());
    snapshot.statistics.context_timestamp_ms = Some(1_000);
    snapshot.statistics.budget = Some(conversation::compaction::Budget {
        model: "selected-model".into(),
        context_limit: 4096,
        trigger_tokens: 2048,
        output_reserve: 2048,
        recent_tokens: 512,
        summary_input: 3584,
        summary_output: 512,
    });
    snapshot.statistics.last_compaction = Some(conversation::compaction::Record {
        id: "summary-before-page".into(),
        sequence: 1,
        turn: TurnId::new(),
        turn_sequence: 1,
        timestamp_ms: 900,
        outcome: conversation::compaction::Outcome::Succeeded,
    });
    projection
        .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
        .unwrap();
    let mut updated = snapshot.statistics.clone();
    updated.responses += 1;
    updated.observations.calls += 1;
    updated.observations.phases.active_us = Some(6_000);
    updated.context_tokens = Some(26);
    updated.context_limit = Some(2_048);
    updated.first_token_us = None;
    updated.context_model = Some("selected-model".into());
    updated.context_timestamp_ms = Some(2_000);
    updated.usage.as_mut().unwrap().input += 10;
    let event = frame(&snapshot, 11, Change::Statistics(Box::new(updated.clone())));
    projection.apply(1, event.clone()).unwrap();
    assert_eq!(projection.apply(1, event.clone()).unwrap(), Apply::Ignored);
    assert!(Arc::ptr_eq(
        &projection.snapshot().unwrap().page,
        &snapshot.page
    ));
    projection
        .merge_history(
            1,
            None,
            conversation::History {
                sequence: 10,
                page: (*snapshot.page).clone(),
                missing: Vec::new(),
            },
        )
        .unwrap();
    assert_eq!(projection.snapshot().unwrap().statistics, updated);
    projection.reconnect(2).unwrap();
    snapshot.sequence = 0;
    snapshot.page = Arc::new(Page {
        revision: 2,
        ..(*snapshot.page).clone()
    });
    snapshot.statistics = Statistics::default();
    projection
        .apply(2, Update::ConversationSnapshot(Box::new(snapshot.clone())))
        .unwrap();
    assert_eq!(projection.apply(1, event).unwrap(), Apply::Ignored);
    assert_eq!(
        projection.snapshot().unwrap().statistics,
        Statistics::default()
    );
}

#[test]
fn leaves_old_context_limits_unknown() {
    let mut value = serde_json::to_value(Statistics {
        context_tokens: Some(100),
        ..Default::default()
    })
    .unwrap();
    value.as_object_mut().unwrap().remove("context_limit");
    value.as_object_mut().unwrap().remove("context_estimate");
    for field in [
        "budget",
        "context_model",
        "context_timestamp_ms",
        "last_compaction",
    ] {
        value.as_object_mut().unwrap().remove(field);
    }
    let statistics: Statistics = serde_json::from_value(value).unwrap();
    assert_eq!(statistics.context_tokens, Some(100));
    assert_eq!(statistics.context_limit, None);
    assert_eq!(statistics.context_estimate, None);
    assert_eq!(statistics.budget, None);
    assert_eq!(statistics.context_model, None);
    assert_eq!(statistics.context_timestamp_ms, None);
    assert_eq!(statistics.last_compaction, None);
}

#[test]
fn replaces_estimates_only_with_node_observation() {
    let (mut projection, mut snapshot, _, _) = fixture();
    snapshot.statistics.context_estimate = Some(300);
    snapshot.statistics.context_limit = Some(1024);
    snapshot.statistics.context_model = Some("summarized-model".into());
    projection
        .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
        .unwrap();
    let mut measured = snapshot.statistics.clone();
    measured.context_estimate = None;
    measured.context_tokens = Some(450);
    measured.context_limit = Some(4096);
    measured.context_model = Some("next-model".into());
    let event = frame(
        &snapshot,
        11,
        Change::Statistics(Box::new(measured.clone())),
    );
    projection.apply(1, event.clone()).unwrap();
    assert_eq!(projection.snapshot().unwrap().statistics, measured);
    assert_eq!(projection.apply(1, event).unwrap(), Apply::Ignored);
}
