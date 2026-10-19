use super::*;
use core::prelude::v1::test;
use sailry_protocol::conversation::Page;

fn history(generation: u64, sequence: u64) -> History {
    let turn = TurnId::new();
    History {
        connected: true,
        generation,
        live_start: Some(sequence),
        snapshot: Some(Arc::new(sailry_protocol::conversation::Snapshot {
            node: sailry_protocol::NodeId([0; 32]),
            sequence,
            page: Arc::new(Page {
                session: SessionId::new(),
                revision: 1,
                entries: vec![sailry_protocol::conversation::Entry {
                    sequence,
                    id: format!("warning-{sequence}"),
                    turn,
                    author: "system".into(),
                    branch: String::new(),
                    timestamp_ms: 0,
                    parts: vec![Part::Resource(
                        serde_json::json!({"type":"operation_warning","code":"context_summary_failed","turn":turn}),
                    )],
                    citations: vec![],
                    search_suggestions: None,
                    usage: None,
                }],
                runs: vec![],
                queue: Default::default(),
                approvals: vec![],
                questions: vec![],
                children: vec![],
                background: vec![],
                next_before: None,
            }),
            missing: vec![],
            drafts: vec![],
            statistics: Default::default(),
            artifacts: Default::default(),
        })),
        ..Default::default()
    }
}

fn snapshot(generation: u64, sequence: u64) -> History {
    let mut value = history(generation, sequence);
    value.live_start = None;
    value
}

#[test]
fn seeds_snapshots_without_replay() {
    let mut cursor = Cursor::default();
    assert!(cursor.accept(&snapshot(1, 10)).is_empty());
    assert_eq!(cursor.accept(&history(1, 11)), vec!["warning-11"]);
    assert!(cursor.accept(&history(1, 11)).is_empty());
    assert!(cursor.accept(&history(1, 3)).is_empty());
    assert!(cursor.accept(&snapshot(2, 12)).is_empty());
    assert_eq!(cursor.accept(&history(2, 13)), vec!["warning-13"]);
    let mut disconnected = history(2, 14);
    disconnected.connected = false;
    assert!(cursor.accept(&disconnected).is_empty());
    assert!(cursor.accept(&snapshot(2, 15)).is_empty());
    assert_eq!(cursor.accept(&history(2, 16)), vec!["warning-16"]);
}

#[test]
fn allows_only_safe_system_entries() {
    let mut cursor = Cursor::default();
    assert!(cursor.accept(&snapshot(1, 1)).is_empty());
    let mut next = history(1, 2);
    Arc::make_mut(&mut Arc::make_mut(next.snapshot.as_mut().unwrap()).page).entries[0].author =
        "model".into();
    assert!(cursor.accept(&next).is_empty());
    let mut next = history(1, 3);
    Arc::make_mut(&mut Arc::make_mut(next.snapshot.as_mut().unwrap()).page).entries[0].parts =
        vec![Part::Resource(
            serde_json::json!({"type":"operation_warning","code":"unsafe_unknown"}),
        )];
    assert!(cursor.accept(&next).is_empty());
}

#[test]
fn ignores_loaded_older_pages() {
    let mut cursor = Cursor::default();
    let mut empty = history(1, 1);
    Arc::make_mut(&mut Arc::make_mut(empty.snapshot.as_mut().unwrap()).page)
        .entries
        .clear();
    empty.live_start = None;
    assert!(cursor.accept(&empty).is_empty());
    let mut older = history(1, 10);
    older.live_start = None;
    assert!(cursor.accept(&older).is_empty());
    assert_eq!(cursor.accept(&history(1, 20)), vec!["warning-20"]);
}

#[test]
fn emits_coalesced_live_warnings() {
    let mut cursor = Cursor::default();
    let mut value = history(1, 11);
    let previous = snapshot(1, 10).snapshot.unwrap().page.entries[0].clone();
    Arc::make_mut(&mut Arc::make_mut(value.snapshot.as_mut().unwrap()).page)
        .entries
        .insert(0, previous);
    assert_eq!(cursor.accept(&value), vec!["warning-11"]);
    assert!(cursor.accept(&value).is_empty());
    assert!(cursor.accept(&snapshot(2, 12)).is_empty());
    assert_eq!(cursor.accept(&history(2, 13)), vec!["warning-13"]);
}
