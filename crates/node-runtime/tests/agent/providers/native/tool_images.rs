use super::*;
use goose_provider_types::{
    base::{Provider, collect_stream},
    conversation::message::Message,
    model::ModelConfig,
};
use goose_providers::{
    api_client::{ApiClient, AuthMethod},
    openai::OpenAiProviderBuilder,
    openai_compatible::OpenAiCompatibleProvider,
};
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock};
use serde_json::json;

#[tokio::test]
async fn preserves_parallel_image_results() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let server = Server::start(api, Reply::Text).await;
        let client = ApiClient::new_with_tls(
            server.endpoint.trim_end_matches("/v1").to_owned(),
            AuthMethod::BearerToken(server::KEY.into()),
            None,
        )
        .unwrap();
        let provider: Arc<dyn Provider> = if api == ModelApi::ChatCompletions {
            Arc::new(OpenAiCompatibleProvider::new(
                "deepseek".into(),
                client,
                "v1/".into(),
            ))
        } else {
            Arc::new(
                OpenAiProviderBuilder::new(client)
                    .base_path("v1/responses")
                    .build(),
            )
        };
        let calls = Message::assistant()
            .with_tool_request("capture", Ok(CallToolRequestParams::new("capture")))
            .with_tool_request("read", Ok(CallToolRequestParams::new("read")));
        let response = |id: &str, image: bool| {
            let mut result = CallToolResult::structured(json!({"ok":true}));
            if image {
                result
                    .content
                    .push(ContentBlock::image("AQID", "image/png"));
            }
            Message::user().with_tool_response(id, Ok(result))
        };
        let contents = vec![
            Message::user().with_text("Inspect the tool images"),
            calls,
            response("capture", true),
            response("read", false),
            Message::assistant()
                .with_thinking("Both tools completed", "")
                .with_text("Observed"),
            Message::user().with_text("Continue"),
        ];
        let canonical = serde_json::to_value(&contents).unwrap();
        let config = ModelConfig::new("fixture-a").with_vision_support(true);
        collect_stream(provider.stream(&config, "", &contents, &[]).await.unwrap())
            .await
            .unwrap();
        assert_eq!(serde_json::to_value(&contents).unwrap(), canonical);
        let requests = server.requests.lock().unwrap();
        let body = &requests[0].body;
        if api == ModelApi::ChatCompletions {
            let messages = body["messages"].as_array().unwrap();
            let outputs: Vec<_> = messages
                .iter()
                .filter(|message| message["role"] == "tool")
                .collect();
            assert_eq!(outputs.len(), 2);
            assert_eq!(outputs[0]["tool_call_id"], "capture");
            assert_eq!(outputs[1]["tool_call_id"], "read");
            assert!(
                outputs
                    .iter()
                    .all(|output| output["content"].as_str().unwrap().contains("true"))
            );
            assert!(messages.iter().any(|message| {
                message["role"] == "user"
                    && message["content"]
                        .to_string()
                        .contains("data:image/png;base64,AQID")
            }));
            let observed = messages
                .iter()
                .find(|message| message["content"] == "Observed")
                .unwrap();
            assert_eq!(observed["reasoning_content"], "Both tools completed");
            assert_eq!(messages.last().unwrap()["content"], "Continue");
        } else {
            let outputs: Vec<_> = body["input"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|item| item["type"] == "function_call_output")
                .collect();
            assert_eq!(outputs.len(), 2);
            assert_eq!(outputs[0]["call_id"], "capture");
            assert_eq!(outputs[1]["call_id"], "read");
            assert!(
                outputs[0]["output"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|part| part["type"] == "input_image"
                        && part["image_url"] == "data:image/png;base64,AQID")
            );
            assert!(!outputs[1]["output"].to_string().contains("input_image"));
        }
    }
}
