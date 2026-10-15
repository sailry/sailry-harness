use super::*;
use core::prelude::v1::test;

#[test]
fn preserves_intrinsic_defaults() {
    let decoration: Decoration = serde_json::from_value(json!({
        "badges": [{"text": "+3", "tone": "success"}]
    }))
    .unwrap();
    assert_eq!(decoration.badges_width, None);
    assert!(!decoration.group);
}
