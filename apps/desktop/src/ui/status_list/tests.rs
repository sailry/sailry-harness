use super::*;
use core::prelude::v1::test;

#[test]
fn preserves_unfinished_static_rows() {
    let mut row: Row = serde_json::from_value(serde_json::json!({
        "id": "unfinished",
        "text": "Inspect the changes",
        "state": "in_progress"
    }))
    .unwrap();
    assert!(row.spinning());
    row.animated = false;
    assert!(!row.spinning());
    assert!(row.state == State::InProgress);
    assert_eq!(row.text, "Inspect the changes");
    row.state = State::Completed;
    row.animated = true;
    assert!(!row.spinning());
}
