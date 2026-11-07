use super::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};

#[tokio::test]
async fn routes_resource_models() {
    for (name, reply, expected) in [
        (
            "gpt-6.1-sol",
            fixtures::responses(false),
            "/openai/v1/responses",
        ),
        (
            "claude-opus-4-8",
            fixtures::anthropic(false),
            "/anthropic/v1/messages",
        ),
        (
            "partner-model",
            fixtures::chat(),
            "/openai/v1/chat/completions",
        ),
    ] {
        let server = Server::start(vec![reply]).await;
        let address = url::Url::parse(&server.endpoint)
            .unwrap()
            .socket_addrs(|| None)
            .unwrap()[0];
        let endpoint = format!("http://fixture.services.ai.azure.com:{}", address.port());
        let provider = provider(ModelApi::AzureAi, endpoint);
        let mut model = native(&provider, name, "foundry-key", HeaderMap::new());
        model.client = reqwest::Client::builder()
            .no_proxy()
            .resolve("fixture.services.ai.azure.com", address)
            .build()
            .unwrap();
        collect(&model, request(name), CancellationToken::new())
            .await
            .unwrap();
        let captured = server.finish().await;
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].path, expected, "{name}");
        assert_eq!(captured[0].headers["api-key"], "foundry-key");
        assert_eq!(captured[0].body["model"], name);
    }
}

#[tokio::test]
async fn routes_project_deployment_metadata() {
    for (publisher, underlying, reply, expected) in [
        (
            "OpenAI",
            "gpt-6.1-sol",
            fixtures::responses(false),
            "/api/projects/captured/openai/v1/responses",
        ),
        (
            "Anthropic",
            "claude-opus-4-8",
            fixtures::anthropic(false),
            "/anthropic/v1/messages",
        ),
        (
            "Partner",
            "partner-model",
            fixtures::chat(),
            "/api/projects/captured/openai/v1/chat/completions",
        ),
    ] {
        let catalog = json!({"value":[{
            "name":"deployment","modelName":underlying,"modelPublisher":publisher
        }]});
        let server = Project::start(catalog, reply).await;
        let provider = provider(
            ModelApi::AzureAi,
            format!("{}/api/projects/captured", server.endpoint),
        );
        let model = native(&provider, "deployment", "foundry-key", HeaderMap::new());
        let mut input = request("deployment");
        if publisher == "Anthropic" {
            input.effort = sailry_protocol::Effort::XHigh;
        }
        collect(&model, input, CancellationToken::new())
            .await
            .unwrap();
        let captured = server.finish().await;
        assert_eq!(captured.len(), 2);
        assert_eq!(
            captured[0].path,
            "/api/projects/captured/deployments?api-version=v1"
        );
        assert_eq!(captured[1].path, expected, "{publisher}");
        for sent in &captured {
            assert_eq!(sent.headers["api-key"], "foundry-key");
        }
        assert_eq!(captured[1].body["model"], "deployment");
        if publisher == "Anthropic" {
            assert_eq!(captured[1].body["thinking"]["type"], "adaptive");
            assert_eq!(captured[1].body["output_config"]["effort"], "xhigh");
        }
    }
}

#[tokio::test]
async fn follows_project_pages_and_strips_root() {
    let server = Project::pages(
        vec![
            json!({
                "value":[{"name":"other","modelName":"gpt-4","modelPublisher":"OpenAI"}],
                "nextLink":"/gateway/api/projects/captured/deployments?page=2"
            }),
            json!({"value":[{
                "name":"deployment","modelName":"claude-opus-4-8","modelPublisher":"Anthropic"
            }]}),
        ],
        fixtures::anthropic(false),
    )
    .await;
    let provider = provider(
        ModelApi::AzureAi,
        format!("{}/gateway/api/projects/captured", server.endpoint),
    );
    let model = native(&provider, "deployment", "foundry-key", HeaderMap::new());
    collect(&model, request("deployment"), CancellationToken::new())
        .await
        .unwrap();
    let captured = server.finish().await;
    assert_eq!(captured.len(), 3);
    assert_eq!(
        captured[0].path,
        "/gateway/api/projects/captured/deployments?api-version=v1"
    );
    assert_eq!(
        captured[1].path,
        "/gateway/api/projects/captured/deployments?page=2&api-version=v1"
    );
    assert_eq!(captured[2].path, "/gateway/anthropic/v1/messages");
    assert_eq!(captured[2].body["model"], "deployment");
    assert!(
        captured
            .iter()
            .all(|request| request.headers["api-key"] == "foundry-key")
    );
}

struct Project {
    endpoint: String,
    requests: Arc<Mutex<Vec<server::Request>>>,
    task: Option<JoinHandle<()>>,
}

impl Project {
    async fn start(catalog: Value, reply: String) -> Self {
        Self::pages(vec![catalog], reply).await
    }

    async fn pages(catalogs: Vec<Value>, reply: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let task = tokio::spawn(async move {
            let replies = catalogs
                .into_iter()
                .map(|catalog| ("application/json", catalog.to_string()))
                .chain(std::iter::once(("text/event-stream", reply)));
            for (content_type, body) in replies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut input = Vec::new();
                let boundary = loop {
                    let mut chunk = [0; 4096];
                    let received = socket.read(&mut chunk).await.unwrap();
                    assert!(received > 0, "request ended before headers");
                    input.extend_from_slice(&chunk[..received]);
                    if let Some(position) = input.windows(4).position(|bytes| bytes == b"\r\n\r\n")
                    {
                        break position + 4;
                    }
                };
                let encoded = std::str::from_utf8(&input[..boundary]).unwrap();
                let mut lines = encoded.lines();
                let mut first = lines.next().unwrap().split_whitespace();
                assert_eq!(
                    first.next().unwrap(),
                    if content_type == "application/json" {
                        "GET"
                    } else {
                        "POST"
                    }
                );
                let path = first.next().unwrap().to_owned();
                let headers = lines
                    .filter_map(|line| line.split_once(':'))
                    .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
                    .collect::<BTreeMap<_, _>>();
                let length = headers
                    .get("content-length")
                    .map_or(0, |length| length.parse::<usize>().unwrap());
                while input.len() < boundary + length {
                    let mut chunk = [0; 4096];
                    let received = socket.read(&mut chunk).await.unwrap();
                    assert!(received > 0, "request ended before body");
                    input.extend_from_slice(&chunk[..received]);
                }
                captured.lock().unwrap().push(server::Request {
                    path,
                    headers,
                    body: if length == 0 {
                        Value::Null
                    } else {
                        serde_json::from_slice(&input[boundary..boundary + length]).unwrap()
                    },
                });
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        Self {
            endpoint,
            requests,
            task: Some(task),
        }
    }

    async fn finish(mut self) -> Vec<server::Request> {
        tokio::time::timeout(Duration::from_secs(10), self.task.take().unwrap())
            .await
            .unwrap()
            .unwrap();
        std::mem::take(&mut *self.requests.lock().unwrap())
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
