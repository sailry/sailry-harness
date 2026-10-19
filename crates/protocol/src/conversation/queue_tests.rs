use super::*;
use serde_json::{Value, json};

fn receipt() -> Value {
    json!({"Ok": {
        "kind": "queue",
        "data": {
            "session": SessionId::new(),
            "queue": {"revision": 3, "paused": true, "items": [{
                "turn": TurnId::new(), "kind": "task", "revision": 2,
                "config_revision": 1, "ready": false, "guidance": null,
                "preview": "Retained 中文 input", "truncated": false, "attachments": []
            }]},
            "runs": []
        }
    }})
}

#[test]
fn retains_unrecorded_input_identity() {
    let mut before = receipt();
    let decoded: Result<crate::Output, Fault> = serde_json::from_value(before.clone()).unwrap();
    let Ok(crate::Output::Queue(update)) = &decoded else {
        panic!("queue receipt expected");
    };
    assert_eq!(update.queue.items[0].request, None);
    assert!(!update.queue.items[0].notice);
    before["Ok"]["data"]["queue"]["items"][0]["notice"] = json!(false);
    assert_eq!(serde_json::to_value(decoded).unwrap(), before);
    assert_eq!(crate::VERSION, 1);
}

#[test]
fn preserves_recorded_input_identity() {
    let mut before = receipt();
    let request = crate::RequestId::new();
    before["Ok"]["data"]["queue"]["items"][0]["request"] = json!(request);
    let decoded: Result<crate::Output, Fault> = serde_json::from_value(before.clone()).unwrap();
    let Ok(crate::Output::Queue(update)) = &decoded else {
        panic!("queue receipt expected");
    };
    assert_eq!(update.queue.items[0].request, Some(request));
    assert!(!update.queue.items[0].notice);
    before["Ok"]["data"]["queue"]["items"][0]["notice"] = json!(false);
    assert_eq!(serde_json::to_value(decoded).unwrap(), before);
}
