use super::*;

#[test]
fn pages_calls_with_stable_totals() {
    let mut fixture = Fixture::new();
    let original = session(&mut fixture);
    let turn = fixture.append((0..45).map(|index| event(START + index % 3, index as i32)));
    let baseline = report(&fixture, &query());
    assert_eq!(baseline.totals.responses, 45);
    assert_eq!(baseline.requests.items.len(), PAGE_SIZE);
    assert!(baseline.requests.has_more);
    let mut selection = query();
    let mut calls = Vec::new();
    loop {
        let page = report(&fixture, &selection);
        assert_eq!(page.totals, baseline.totals);
        assert_eq!(page.days, baseline.days);
        assert_eq!(page.groups, baseline.groups);
        assert_eq!(page.resources, baseline.resources);
        selection.before = page.requests.items.last().map(|call| call.position);
        calls.extend(page.requests.items);
        if !page.requests.has_more {
            break;
        }
    }
    assert_eq!(calls.len(), 45);
    assert!(
        calls
            .windows(2)
            .all(|pair| pair[0].position > pair[1].position)
    );
    assert_eq!(
        calls.iter().map(|call| call.tokens.input).sum::<u64>(),
        (0..45).sum::<u64>()
    );
    for call in &calls {
        assert_eq!(call.session, original.id);
        assert_eq!(call.turn, turn);
        assert_eq!(call.project, original.project);
        assert_eq!(call.worktree, original.worktree);
        assert_eq!(call.provider, original.config.provider);
        assert_eq!(call.model, original.config.model);
        assert_eq!(call.position.node, fixture.database.node);
        assert_eq!(call.usd_micros, None);
        assert_eq!(call.elapsed_us, None);
        assert_eq!(call.first_token_us, None);
        assert!(!call.provider_name.is_empty());
        assert!(!call.scope_name.is_empty());
    }
    fixture.append([event(START + DAY, 100)]);
    let tail = report(&fixture, &selection);
    assert_eq!(tail.totals.responses, 46);
    assert!(tail.requests.items.is_empty());
    assert!(!tail.requests.has_more);
    assert_eq!(
        report(&fixture, &query()).requests.items[0].tokens.input,
        100
    );
}

#[test]
fn orders_equal_timestamps_across_nodes() {
    let mut fixture = Fixture::new();
    fixture.append([event(START, 10), event(START, 20)]);
    let mut selection = query();
    selection.before = Some(Position {
        timestamp_ms: START,
        node: NodeId([255; 32]),
        sequence: 0,
    });
    assert_eq!(report(&fixture, &selection).requests.items.len(), 2);
    selection.before.as_mut().unwrap().node = NodeId([0; 32]);
    assert!(report(&fixture, &selection).requests.items.is_empty());
    selection.before.as_mut().unwrap().timestamp_ms = START - 1;
    assert!(
        read(
            &fixture.database.connection,
            fixture.database.node,
            &selection
        )
        .is_err()
    );
}

#[test]
fn deduplicates_forked_calls() {
    let mut fixture = Fixture::new();
    let entry = event(START, 10);
    let turn = fixture.append([entry.clone(), entry]);
    let original = report(&fixture, &query());
    assert_eq!(original.requests.items.len(), 1);
    command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: fixture.session,
            through: turn,
            expected_revision: 1,
        },
    );
    assert_eq!(report(&fixture, &query()).requests, original.requests);
}

#[test]
fn preserves_latency_meaning() {
    let mut fixture = Fixture::new();
    for (index, first) in [
        serde_json::Value::Null,
        serde_json::json!(12500),
        serde_json::json!(30000),
        serde_json::json!(-1),
    ]
    .into_iter()
    .enumerate()
    {
        let mut entry = event(START + index as i64, 10);
        entry.llm_response.provider_metadata = Some(serde_json::json!({
            "sailry_timing": { "elapsed_us": 20000, "first_token_us": first }
        }));
        fixture.append([entry]);
    }
    let calls = report(&fixture, &query()).requests.items;
    assert_eq!(
        calls
            .iter()
            .map(|call| call.first_token_us)
            .collect::<Vec<_>>(),
        [None, None, Some(12500), None]
    );
    assert!(calls.iter().all(|call| call.elapsed_us == Some(20000)));
}
