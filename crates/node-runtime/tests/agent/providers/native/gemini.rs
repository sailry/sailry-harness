use super::*;
use adk_core::{Content, FunctionResponseData, GenerateContentConfig, Llm, LlmRequest, Part};
use adk_model::gemini::GeminiModel;
use futures::StreamExt;
use serde_json::{Value, json};

const POLICY: &str = "Shared policy\n Preserve spacing 中文 🙂  ";
const DETAILS: &str = "Apply the current task rules";
const CONTEXT: &str = "Use the selected project";
const TASK: &str = "First user task 🙂";

fn request() -> LlmRequest {
    LlmRequest::new(
        "fixture-a",
        vec![
            Content::new("system").with_text(POLICY).with_text(DETAILS),
            Content::new("user").with_text(TASK),
            Content::new("system").with_text(CONTEXT),
        ],
    )
}

async fn envelope(request: LlmRequest, streaming: bool) -> Value {
    let reply = if streaming {
        Reply::Text
    } else {
        let body = json!({
            "candidates": [{
                "content": {"role": "model", "parts": [{"text": server::ANSWER}]},
                "finishReason": "STOP", "index": 0,
            }],
        })
        .to_string();
        Reply::Raw(Arc::new(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .into_bytes(),
        ))
    };
    let server = Server::start(ModelApi::Gemini, reply).await;
    let model =
        GeminiModel::new_with_base_url(server::KEY, "fixture-a", format!("{}/", server.endpoint))
            .unwrap();
    let mut responses = model.generate_content(request, streaming).await.unwrap();
    let mut answer = String::new();
    while let Some(response) = responses.next().await {
        let response = response.unwrap();
        assert!(response.error_message.is_none());
        if let Some(content) = response.content {
            for part in content.parts {
                if let Part::Text { text } = part {
                    answer.push_str(&text);
                }
            }
        }
    }
    assert_eq!(answer, server::ANSWER);
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let path = if streaming {
        "/v1/models/fixture-a:streamGenerateContent?alt=sse"
    } else {
        "/v1/models/fixture-a:generateContent"
    };
    assert_eq!(requests[0].path, path);
    assert_eq!(requests[0].headers["x-goog-api-key"], server::KEY);
    requests[0].body.clone()
}

#[tokio::test]
async fn preserves_first_turn_instructions() {
    for streaming in [false, true] {
        let body = envelope(request(), streaming).await;
        assert!(body.get("systemInstruction").is_none());
        assert_eq!(
            body["contents"],
            json!([
                {"role": "user", "parts": [{"text": POLICY}, {"text": DETAILS}]},
                {"role": "user", "parts": [{"text": TASK}]},
                {"role": "user", "parts": [{"text": CONTEXT}]},
            ]),
        );
        assert!(body.get("generationConfig").is_none());
    }
}

#[tokio::test]
async fn preserves_continuation_and_options() {
    for streaming in [false, true] {
        let mut request = request();
        request.contents.extend([
            Content::new("model").with_text("First answer"),
            Content::new("user").with_text("Continue the task"),
            Content {
                role: "model".into(),
                parts: vec![Part::FunctionCall {
                    name: "read_file".into(),
                    args: json!({"path": "native.txt"}),
                    id: Some("call-1".into()),
                    thought_signature: Some("signature-1".into()),
                }],
            },
            Content {
                role: "function".into(),
                parts: vec![Part::FunctionResponse {
                    function_response: FunctionResponseData::new(
                        "read_file",
                        json!({"content": "Tool result 🙂"}),
                    ),
                    id: Some("call-1".into()),
                    annotations: None,
                }],
            },
        ]);
        request.config = Some(GenerateContentConfig {
            temperature: Some(0.25),
            top_p: Some(0.75),
            top_k: Some(7),
            max_output_tokens: Some(128),
            response_schema: Some(json!({"type": "object"})),
            ..Default::default()
        });
        request.tools.insert(
            "read_file".into(),
            json!({"description": "Read the fixture", "parameters": {
                "type": "object", "properties": {"path": {"type": "string"}},
                "required": ["path"],
            }}),
        );
        let body = envelope(request, streaming).await;
        assert!(body.get("systemInstruction").is_none());
        assert_eq!(
            body["contents"],
            json!([
                {"role": "user", "parts": [{"text": POLICY}, {"text": DETAILS}]},
                {"role": "user", "parts": [{"text": TASK}]},
                {"role": "user", "parts": [{"text": CONTEXT}]},
                {"role": "model", "parts": [{"text": "First answer"}]},
                {"role": "user", "parts": [{"text": "Continue the task"}]},
                {"role": "model", "parts": [{
                    "functionCall": {"name": "read_file", "args": {"path": "native.txt"}, "id": "call-1"},
                    "thoughtSignature": "signature-1",
                }]},
                {"role": "user", "parts": [{
                    "functionResponse": {"name": "read_file", "id": "call-1", "response": {"content": "Tool result 🙂"}},
                    "thoughtSignature": "signature-1",
                }]},
            ])
        );
        let config = &body["generationConfig"];
        assert_eq!(config["temperature"], 0.25);
        assert_eq!(config["topP"], 0.75);
        assert_eq!(config["topK"], 7);
        assert_eq!(config["maxOutputTokens"], 128);
        assert_eq!(config["responseSchema"], json!({"type": "object"}));
        assert_eq!(config["responseMimeType"], "application/json");
        let declaration = &body["tools"][0]["functionDeclarations"][0];
        assert_eq!(declaration["name"], "read_file");
        assert_eq!(declaration["description"], "Read the fixture");
        assert_eq!(declaration["parameters"]["required"], json!(["path"]));
    }
}
