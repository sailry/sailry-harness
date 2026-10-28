use super::*;

#[test]
fn rejects_unknown_fields() {
    assert!(
        serde_json::from_str::<Frame>(r#"{"command":"node_hello","version":1,"fallback":true}"#)
            .is_err()
    );
}
