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
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "retains_unrecorded_input_identity",
            (|scenario: &'static str| {
                let mut before = receipt();
                let decoded: Result<crate::Output, Fault> =
                    serde_json::from_value(before.clone()).unwrap();
                let Ok(crate::Output::Queue(update)) = &decoded else {
                    panic!("{scenario}: queue receipt expected");
                };
                assert_eq!(update.queue.items[0].request, None, "{scenario}");
                assert!(!update.queue.items[0].notice, "{scenario}");
                before["Ok"]["data"]["queue"]["items"][0]["notice"] = json!(false);
                assert_eq!(serde_json::to_value(decoded).unwrap(), before, "{scenario}");
            }) as fn(&'static str),
        ),
        (
            "preserves_recorded_input_identity",
            (|scenario: &'static str| {
                let mut before = receipt();
                let request = crate::RequestId::new();
                before["Ok"]["data"]["queue"]["items"][0]["request"] = json!(request);
                let decoded: Result<crate::Output, Fault> =
                    serde_json::from_value(before.clone()).unwrap();
                let Ok(crate::Output::Queue(update)) = &decoded else {
                    panic!("{scenario}: queue receipt expected");
                };
                assert_eq!(update.queue.items[0].request, Some(request), "{scenario}");
                assert!(!update.queue.items[0].notice, "{scenario}");
                before["Ok"]["data"]["queue"]["items"][0]["notice"] = json!(false);
                assert_eq!(serde_json::to_value(decoded).unwrap(), before, "{scenario}");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
