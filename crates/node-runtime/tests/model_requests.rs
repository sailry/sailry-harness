//! GDK stream behavior against an isolated HTTP server, without account access.
use crate::discovery_support::{Reply, Server};
use futures::StreamExt;
use goose_provider_types::{base::Provider, conversation::message::Message, model::ModelConfig};
use goose_providers::{
    api_client::{ApiClient, AuthMethod},
    openai::OpenAiProviderBuilder,
};
use sailry_protocol::conversation::ModelApi;

#[tokio::test]
async fn never_restarts_a_partial_stream() {
    let server = Server::start_with_request(ModelApi::Anthropic, |_| {
        let body = concat!(
            "data: {\"type\":\"response.created\",\"sequence_number\":0,\"response\":{\"id\":\"fixture-response\",\"object\":\"response\",\"created_at\":0,\"model\":\"fixture\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"sequence_number\":1,\"item_id\":\"fixture-item\",\"output_index\":0,\"content_index\":0,\"delta\":\"partial\",\"logprobs\":[]}\n\n",
            "data: invalid-json\n\n",
        );
        Reply::Raw(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()))
    }).await;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = ApiClient::new_with_tls(
        server.endpoint.clone(),
        AuthMethod::BearerToken("isolated-key".into()),
        None,
    )
    .unwrap();
    let model = OpenAiProviderBuilder::new(client)
        .base_path("responses")
        .build();
    let config = ModelConfig::new("fixture");
    let contents = [Message::user().with_text("Isolated request")];
    let mut stream = model.stream(&config, "", &contents, &[]).await.unwrap();
    let mut partial = String::new();
    let mut failed = false;
    while let Some(response) = stream.next().await {
        match response {
            Ok((message, _)) => {
                if let Some(message) = message {
                    partial.push_str(&message.as_concat_text());
                }
            }
            Err(_) => {
                failed = true;
                break;
            }
        }
    }
    assert_eq!(partial, "partial");
    assert!(failed);
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}
