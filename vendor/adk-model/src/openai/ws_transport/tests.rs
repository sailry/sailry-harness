use super::*;
use serde_json::json;
#[test]
fn preserves_endpoint_paths() {
    assert_eq!(
        derive_ws_url("https://chatgpt.com/backend-api/codex"),
        "wss://chatgpt.com/backend-api/codex/responses"
    );
    assert_eq!(
        derive_ws_url("http://localhost:8080/v1/"),
        "ws://localhost:8080/v1/responses"
    );
}
#[test]
fn extends_only_the_completed_baseline() {
    let request =
        json!({"model":"fixture", "store":false,"input":[{"role":"user","content":"first"}]});
    let output = json!({"id":"msg_1","status":"completed","role":"assistant","content":"answer"});
    let previous = Continuation {
        request: request.clone(),
        response: "resp_1".into(),
        output: vec![output],
    };
    let mut next = request;
    next["input"].as_array_mut().unwrap().extend([
        json!({"role":"assistant","content":"answer"}),
        json!({"role":"user","content":"next"}),
    ]);
    let incremental = incremental_event(&next, &previous).unwrap();
    assert_eq!(incremental["previous_response_id"], "resp_1");
    assert_eq!(
        incremental["input"],
        json!([{"role":"user","content":"next"}])
    );
    next["model"] = json!("changed");
    assert!(incremental_event(&next, &previous).is_none());
    next["model"] = json!("fixture");
    next["input"][0]["content"] = json!("changed");
    assert!(incremental_event(&next, &previous).is_none());
}
#[test]
fn strips_transport_fields() {
    let event = create_event(
        &json!({"stream":true,"background":false,"previous_response_id":"caller","input":[]}),
    )
    .unwrap();
    assert_eq!(event, json!({"type":"response.create","input":[]}));
}
