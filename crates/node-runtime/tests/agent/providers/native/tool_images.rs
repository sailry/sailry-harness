use super::*;
use adk_core::{Content, FunctionResponseData, InlineDataPart, Llm, LlmRequest, Part as AdkPart};
use adk_model::openai::{OpenAIResponsesClient, OpenAIResponsesConfig};
use adk_model::{OpenAICompatible, OpenAICompatibleConfig, ReasoningReplayField};
use futures::StreamExt;
use serde_json::json;

#[tokio::test]
async fn preserves_parallel_image_results() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let server = Server::start(api, Reply::Text).await;
        let model: Arc<dyn Llm> = if api == ModelApi::ChatCompletions {
            Arc::new(
                OpenAICompatible::new(
                    OpenAICompatibleConfig::new(server::KEY, "fixture-a")
                        .with_base_url(&server.endpoint),
                )
                .unwrap()
                .with_reasoning_replay_field(ReasoningReplayField::ReasoningContent),
            )
        } else {
            Arc::new(
                OpenAIResponsesClient::new(
                    OpenAIResponsesConfig::new(server::KEY, "fixture-a")
                        .with_base_url(&server.endpoint),
                )
                .unwrap(),
            )
        };
        let mut calls = Content::new("assistant");
        for id in ["capture", "read"] {
            calls.parts.push(AdkPart::FunctionCall {
                id: Some(id.into()),
                name: id.into(),
                args: json!({}),
                thought_signature: None,
            });
        }
        let response = |name: &str, images: Vec<InlineDataPart>| Content {
            role: "tool".into(),
            parts: vec![AdkPart::FunctionResponse {
                id: Some(name.into()),
                function_response: FunctionResponseData::with_inline_data(
                    name,
                    json!({"ok":true}),
                    images,
                ),
                annotations: None,
            }],
        };
        let image = InlineDataPart {
            mime_type: "image/png".into(),
            data: vec![1, 2, 3],
            uri: None,
            annotations: None,
        };
        let contents = vec![
            Content::new("user").with_text("Inspect the tool images"),
            calls,
            response("capture", vec![image]),
            response("read", vec![]),
            Content::new("assistant")
                .with_thinking("Both tools completed")
                .with_text("Observed"),
            Content::new("user").with_text("Continue"),
        ];
        let canonical = serde_json::to_value(&contents).unwrap();
        let mut stream = model
            .generate_content(LlmRequest::new("fixture-a", contents.clone()), true)
            .await
            .unwrap();
        while let Some(response) = stream.next().await {
            response.unwrap();
        }
        assert_eq!(serde_json::to_value(&contents).unwrap(), canonical);
        let requests = server.requests.lock().unwrap();
        let body = &requests[0].body;
        if api == ModelApi::ChatCompletions {
            let messages = body["messages"].as_array().unwrap();
            assert_eq!(messages[2]["tool_call_id"], "capture");
            assert_eq!(messages[3]["tool_call_id"], "read");
            assert_eq!(messages[4]["role"], "user");
            assert_eq!(
                messages[4]["content"][1]["image_url"]["url"],
                "data:image/png;base64,AQID"
            );
            assert!(
                messages[4]["content"][0]["text"]
                    .as_str()
                    .unwrap()
                    .contains("capture")
            );
            assert_eq!(messages[5]["reasoning_content"], "Both tools completed");
            assert!(messages[4].get("reasoning_content").is_none());
            assert_eq!(messages[6]["content"], "Continue");
        } else {
            let input = body["input"].as_array().unwrap();
            let tools: Vec<_> = input
                .iter()
                .enumerate()
                .filter(|(_, item)| item["type"] == "function_call_output")
                .collect();
            assert_eq!(tools.len(), 2);
            assert_eq!(tools[0].1["call_id"], "capture");
            assert_eq!(tools[1].1["call_id"], "read");
            let (index, image) = input
                .iter()
                .enumerate()
                .find_map(|(index, item)| {
                    item["content"]
                        .as_array()?
                        .iter()
                        .find(|part| part["type"] == "input_image")
                        .map(|image| (index, image))
                })
                .unwrap();
            assert!(index > tools[1].0);
            assert_eq!(image["image_url"], "data:image/png;base64,AQID");
        }
    }
}
