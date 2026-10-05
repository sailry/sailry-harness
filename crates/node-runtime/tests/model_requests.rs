//! Direct ADK transport checks against isolated HTTP servers, without account access.
use crate::discovery_support::{Reply, Server};
use adk_core::{Content, Llm, LlmRequest};
use adk_model::{
    openai::{OpenAIReasoningEffort, OpenAIResponsesClient, OpenAIResponsesConfig},
    retry::RetryConfig,
};
use futures::StreamExt;
use sailry_protocol::conversation::ModelApi;
use std::time::Duration;

#[path = "model_requests/runtime.rs"]
mod runtime;

fn failed(status: u16) -> Reply {
    let body = r#"{"error":{"message":"isolated failure","type":"server_error","code":"fixture"}}"#;
    Reply::Raw(format!(
        "HTTP/1.1 {status} Failure\r\nContent-Type: application/json\r\nContent-Length: {}\r\nRetry-After: 0\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ))
}

async fn run(model: &OpenAIResponsesClient, streaming: bool) {
    let request = LlmRequest::new(
        "fixture",
        vec![Content::new("user").with_text("Isolated request")],
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        match model.generate_content(request, streaming).await {
            Err(_) => {}
            Ok(mut stream) => assert!(stream.next().await.unwrap().is_err()),
        }
    })
    .await
    .unwrap();
}

fn model(server: &Server, mode: &str) -> OpenAIResponsesClient {
    let config = OpenAIResponsesConfig::new("isolated-key", "fixture")
        .with_base_url(&server.endpoint)
        .with_open_responses_mode(mode == "compatible");
    if mode == "max" {
        OpenAIResponsesClient::new_with_reasoning_effort(config, OpenAIReasoningEffort::Max)
    } else {
        OpenAIResponsesClient::new(config)
    }
    .unwrap()
}

#[tokio::test]
async fn disables_hidden_retries() {
    for status in [429, 503] {
        for streaming in [false, true] {
            for mode in ["standard", "compatible", "max"] {
                let server =
                    Server::start_with_request(ModelApi::Anthropic, move |_| failed(status)).await;
                let model = model(&server, mode).with_retry_config(RetryConfig::disabled());
                run(&model, streaming).await;
                assert_eq!(
                    server.requests.lock().unwrap().len(),
                    1,
                    "{mode}, streaming={streaming}, status={status}"
                );
            }
        }
    }
}

#[tokio::test]
async fn honors_explicit_retry_limit() {
    for streaming in [false, true] {
        for mode in ["standard", "compatible", "max"] {
            let server = Server::start_with_request(ModelApi::Anthropic, |_| failed(503)).await;
            let mut model = model(&server, mode).with_retry_config(
                RetryConfig::default()
                    .with_max_retries(1)
                    .with_initial_delay(Duration::ZERO),
            );
            run(&model, streaming).await;
            assert_eq!(server.requests.lock().unwrap().len(), 2);
            model.set_retry_config(RetryConfig::disabled());
            run(&model, streaming).await;
            assert_eq!(server.requests.lock().unwrap().len(), 3);
        }
    }
}

#[tokio::test]
async fn never_restarts_a_partial_stream() {
    for mode in ["standard", "compatible", "max"] {
        let server = Server::start_with_request(ModelApi::Anthropic, |_| {
            let body = "data: {\"type\":\"response.output_text.delta\",\"sequence_number\":1,\"item_id\":\"fixture-item\",\"output_index\":0,\"content_index\":0,\"delta\":\"partial\",\"logprobs\":[]}\n\ndata: invalid-json\n\n";
            Reply::Raw(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()))
        }).await;
        let model = model(&server, mode);
        let request = LlmRequest::new(
            "fixture",
            vec![Content::new("user").with_text("Isolated request")],
        );
        let mut stream = model.generate_content(request, true).await.unwrap();
        assert!(stream.next().await.unwrap().unwrap().partial);
        assert!(stream.next().await.unwrap().is_err());
        assert_eq!(server.requests.lock().unwrap().len(), 1);
    }
}
