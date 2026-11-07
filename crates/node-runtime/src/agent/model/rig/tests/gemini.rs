use super::*;
use rmcp::model::CallToolRequestParams;

const MODEL: &str = "gemini-3-flash-preview";

fn call(id: &str) -> Message {
    Message::assistant().with_tool_request(id, Ok(CallToolRequestParams::new("read")))
}

async fn captured(input: Request) -> Value {
    let server = Server::start(vec![fixtures::gemini(false)]).await;
    let provider = provider(ModelApi::Gemini, server.endpoint.clone());
    let model = native(&provider, MODEL, "gemini-key", HeaderMap::new());
    collect(&model, input, CancellationToken::new())
        .await
        .unwrap();
    let requests = server.finish().await;
    assert_eq!(requests.len(), 1);
    requests.into_iter().next().unwrap().body
}

fn parts(body: &Value) -> Vec<&Value> {
    body["contents"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|content| content["parts"].as_array().unwrap())
        .collect()
}

#[tokio::test]
async fn pairs_history_before_assigning_ids() {
    let mut input = request(MODEL);
    input.messages.extend([
        result("orphan"),
        call("call-a"),
        call("call-b"),
        result("call-b"),
    ]);
    let body = captured(input).await;
    let parts = parts(&body);
    let calls: Vec<_> = parts
        .iter()
        .filter(|part| part.get("functionCall").is_some())
        .collect();
    assert_eq!(calls.len(), 2);
    for (part, id) in calls.iter().zip(["call-a", "call-b"]) {
        assert_eq!(part["functionCall"]["id"], id);
        assert_eq!(part["functionCall"]["name"], "read");
        assert_eq!(part["thoughtSignature"], "skip_thought_signature_validator");
    }
    let results: Vec<_> = parts
        .iter()
        .filter_map(|part| part.get("functionResponse"))
        .collect();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["id"], "call-a");
    assert_eq!(
        results[0]["response"]["content"]["text"],
        "No result provided"
    );
    assert_eq!(results[1]["id"], "call-b");
    assert_eq!(results[1]["response"]["content"]["text"], "File contents");
    assert!(!body.to_string().contains("orphan"));
    assert!(!body.to_string().contains("__sailry"));
}

#[tokio::test]
async fn preserves_raw_contents() {
    let mut input = request(MODEL);
    input
        .messages
        .extend([call("history-call"), result("history-call")]);
    let contents = json!([
        {"role":"user","parts":[{"text":"Raw request text"}]},
        {"role":"model","parts":[{
            "functionCall":{"id":"raw-call","name":"read","args":{"path":"raw.txt"}},
            "thoughtSignature":"raw-signature"
        }]},
        {"role":"user","parts":[{
            "functionResponse":{
                "id":"raw-call",
                "name":"read",
                "response":{"content":{"text":"Raw result text"}}
            }
        }]}
    ]);
    input.params.insert("contents".into(), contents.clone());
    let body = captured(input).await;
    assert_eq!(body["contents"], contents);
    assert!(!body.to_string().contains("history-call"));
    assert!(!body.to_string().contains("__sailry"));
}

#[tokio::test]
async fn retains_calls_with_null_tools() {
    let mut input = request(MODEL);
    input.tools.clear();
    input.params.insert("tools".into(), Value::Null);
    input
        .messages
        .extend([call("saved-call"), result("saved-call")]);
    let body = captured(input).await;
    let parts = parts(&body);
    assert!(parts.iter().any(|part| {
        part["functionCall"]["id"] == "saved-call" && part["functionCall"]["name"] == "read"
    }));
    assert!(parts.iter().any(|part| {
        part["functionResponse"]["id"] == "saved-call"
            && part["functionResponse"]["response"]["content"]["text"] == "File contents"
    }));
    assert!(body.get("tools").is_none());
    assert!(!body.to_string().contains("sailry_history"));
    assert!(!body.to_string().contains("__sailry"));
}

#[tokio::test]
async fn replays_calls_from_cached_content() {
    for field in ["cachedContent", "cached_content"] {
        let mut input = request(MODEL);
        input.system.clear();
        input.tools.clear();
        input
            .params
            .insert(field.into(), "cachedContents/fixture".into());
        input
            .messages
            .extend([call("cached-call"), result("cached-call")]);
        let body = captured(input).await;
        assert_eq!(body["cachedContent"], "cachedContents/fixture");
        assert!(body.get("tools").is_none_or(Value::is_null));
        assert!(body.get("toolConfig").is_none());
        assert!(parts(&body).iter().any(|part| {
            part["functionCall"]["id"] == "cached-call" && part["functionCall"]["name"] == "read"
        }));
        assert!(!body.to_string().contains("sailry_history"));
        assert!(!body.to_string().contains("__sailry"));
    }
}
