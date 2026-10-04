use super::*;
use sailry_protocol::conversation::{Statistics, Usage};

#[test]
fn recovers_totals_across_generations() {
    let (mut projection, mut snapshot, _, _) = fixture();
    snapshot.statistics = Statistics {
        turns: 25,
        responses: 30,
        context_tokens: Some(16),
        usage: Some(Usage {
            input: 300,
            output: 60,
            cached_input: 20,
            reasoning: 10,
        }),
        ..Default::default()
    };
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    let mut updated = snapshot.statistics.clone();
    updated.responses += 1;
    updated.context_tokens = Some(26);
    updated.usage.as_mut().unwrap().input += 10;
    let event = frame(&snapshot, 11, Change::Statistics(updated.clone()));
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
        .apply(2, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    assert_eq!(projection.apply(1, event).unwrap(), Apply::Ignored);
    assert_eq!(
        projection.snapshot().unwrap().statistics,
        Statistics::default()
    );
}
