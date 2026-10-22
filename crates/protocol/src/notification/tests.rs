use super::*;

#[test]
fn omitted_details_remain_empty() {
    let draft: Draft = serde_json::from_value(serde_json::json!({
        "title": "Reminder", "message": "Review", "kind": "info", "session": null
    }))
    .unwrap();
    assert!(draft.details.is_none());
}

#[test]
fn details_keep_their_type() {
    let details = Details::Text {
        body: "A completed workflow result".into(),
    };
    let wire = serde_json::to_value(&details).unwrap();
    assert_eq!(wire["type"], "text");
    assert_eq!(serde_json::from_value::<Details>(wire).unwrap(), details);
}
