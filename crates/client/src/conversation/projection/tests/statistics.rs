use super::*;
use sailry_protocol::conversation::{Statistics, Usage};

#[test]
fn recovers_totals_across_generations() {
    let (mut projection, mut snapshot, _, _) = fixture();
    let mut statistics = Statistics {
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
    statistics.observations.calls = 7;
    statistics.observations.phases.active_us = Some(5_000);
    statistics.context_model = Some("observed-model".into());
    statistics.context_timestamp_ms = Some(1_000);
    statistics.budget = Some(conversation::compaction::Budget {
        model: "selected-model".into(),
        context_limit: 4096,
        trigger_tokens: 2048,
        output_reserve: 2048,
        recent_tokens: 512,
        summary_input: 3584,
        summary_output: 512,
    });
    statistics.last_compaction = Some(conversation::compaction::Record {
        id: "summary-before-page".into(),
        sequence: 1,
        turn: TurnId::new(),
        turn_sequence: 1,
        timestamp_ms: 900,
        outcome: conversation::compaction::Outcome::Succeeded,
    });
    snapshot.statistics = Some(statistics.clone());
    projection
        .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
        .unwrap();
    let mut updated = statistics;
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
    assert_eq!(projection.snapshot().unwrap().statistics, Some(updated));
    projection.reconnect(2).unwrap();
    snapshot.sequence = 0;
    snapshot.page = Arc::new(Page {
        revision: 2,
        ..(*snapshot.page).clone()
    });
    snapshot.statistics = None;
    projection
        .apply(2, Update::ConversationSnapshot(Box::new(snapshot.clone())))
        .unwrap();
    assert_eq!(projection.apply(1, event).unwrap(), Apply::Ignored);
    assert_eq!(projection.snapshot().unwrap().statistics, None);
    let corrected = Statistics {
        turns: 1,
        responses: 2,
        ..Default::default()
    };
    projection
        .apply(
            2,
            frame(
                &snapshot,
                1,
                Change::Statistics(Box::new(corrected.clone())),
            ),
        )
        .unwrap();
    assert_eq!(projection.snapshot().unwrap().statistics, Some(corrected));
}

#[test]
fn reports_failure_without_resetting_history() {
    let (mut projection, mut snapshot, _, _) = fixture();
    snapshot.statistics = Some(Statistics::default());
    projection
        .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
        .unwrap();
    assert!(projection.snapshot().unwrap().statistics.is_some());
    let error = Fault::new(ErrorCode::Internal, "statistics read failed");
    let failure = frame(&snapshot, 11, Change::StatisticsFailed(error.clone()));
    assert_eq!(
        projection.apply(1, failure.clone()).unwrap(),
        Apply::Applied
    );
    assert_eq!(projection.statistics_error(), Some(&error));
    assert!(Arc::ptr_eq(
        &projection.snapshot().unwrap().page,
        &snapshot.page
    ));
    assert!(projection.snapshot().unwrap().statistics.is_none());
    assert_eq!(projection.apply(1, failure).unwrap(), Apply::Ignored);
    let statistics = Statistics {
        turns: 25,
        responses: 30,
        ..Default::default()
    };
    let ready = frame(
        &snapshot,
        12,
        Change::Statistics(Box::new(statistics.clone())),
    );
    projection.apply(1, ready).unwrap();
    assert_eq!(projection.snapshot().unwrap().statistics, Some(statistics));
    assert!(projection.statistics_error().is_none());
    assert!(Arc::ptr_eq(
        &projection.snapshot().unwrap().page,
        &snapshot.page
    ));
}

#[test]
fn ignores_failures_from_previous_generations() {
    let (mut projection, mut snapshot, _, _) = fixture();
    projection
        .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
        .unwrap();
    let failure = frame(
        &snapshot,
        11,
        Change::StatisticsFailed(Fault::new(ErrorCode::Internal, "statistics read failed")),
    );
    projection.apply(1, failure.clone()).unwrap();
    assert!(projection.statistics_error().is_some());
    projection.reconnect(2).unwrap();
    snapshot.sequence = 0;
    projection
        .apply(2, Update::ConversationSnapshot(Box::new(snapshot.clone())))
        .unwrap();
    assert!(projection.statistics_error().is_none());
    assert_eq!(projection.apply(1, failure).unwrap(), Apply::Ignored);
    assert!(projection.statistics_error().is_none());
    let statistics = Statistics::default();
    projection
        .apply(
            2,
            frame(
                &snapshot,
                1,
                Change::Statistics(Box::new(statistics.clone())),
            ),
        )
        .unwrap();
    assert_eq!(projection.snapshot().unwrap().statistics, Some(statistics));
    assert!(Arc::ptr_eq(
        &projection.snapshot().unwrap().page,
        &snapshot.page
    ));
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
    snapshot.statistics = Some(Statistics {
        context_estimate: Some(300),
        context_limit: Some(1024),
        context_model: Some("summarized-model".into()),
        ..Default::default()
    });
    projection
        .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
        .unwrap();
    let mut measured = snapshot.statistics.clone().unwrap();
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
    assert_eq!(projection.snapshot().unwrap().statistics, Some(measured));
    assert_eq!(projection.apply(1, event).unwrap(), Apply::Ignored);
}
