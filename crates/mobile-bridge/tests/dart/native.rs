use super::configuration::{config, provider};
use base64::Engine;
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{conversation::ModelApi, *};

#[allow(dead_code)]
#[path = "../../../node-runtime/tests/agent/providers/native/server.rs"]
pub(super) mod server;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn resumes_without_source() {
    for (name, api, model) in [
        ("anthropic", ModelApi::Anthropic, "fixture-a"),
        ("gemini", ModelApi::Gemini, "gemini-2.5-flash"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let server = runtime.block_on(server::Server::start(api, server::Reply::Tool));
        std::fs::write(directory.path().join("native.txt"), "工具内容 🙂").unwrap();
        let (target, session) = runtime.block_on(async {
            let source = Node::start(directory.path().join("source")).await.unwrap();
            let target = Node::start(directory.path().join("target")).await.unwrap();
            source
                .link()
                .pair(target.link().invite().unwrap().ticket())
                .await
                .unwrap();
            let origin = Client::new(source.local());
            let destination = Client::new(target.local());
            let mut selected = provider(&origin, &server.endpoint, model).await;
            selected.api = api;
            selected.models[0].tools = true;
            selected.models[0].vision = true;
            selected.models[0].context = 65_536;
            selected.models[0].output = 4096;
            selected.models[0].reasoning = true;
            selected.models[0].efforts = vec![Effort::Default, Effort::Budget(1024)];
            selected.models[0].default_effort = Effort::Budget(1024);
            let Output::Provider(selected) = origin
                .execute(origin.prepare(Command::PutProvider {
                    expected_revision: selected.revision,
                    provider: selected,
                }))
                .await
                .unwrap()
            else {
                panic!("provider expected")
            };
            let Output::Project(project) = destination
                .execute(destination.prepare(Command::RegisterProject {
                    name: "Native FFI fixture".into(),
                    path: directory.path().to_str().unwrap().into(),
                }))
                .await
                .unwrap()
            else {
                panic!("project expected")
            };
            let mut selection = config(&selected);
            selection.effort = Effort::Budget(1024);
            let Output::Session(session) = origin
                .execute(origin.prepare(Command::CreateSessionAt {
                    target: target.id(),
                    project: Some(project.id),
                    worktree: None,
                    config: Box::new(selection),
                    provider_revision: selected.revision,
                }))
                .await
                .unwrap()
            else {
                panic!("session expected")
            };
            source.shutdown().await.unwrap();
            (target, session)
        });
        let invitation = target.link().invite().unwrap();
        let child = super::command("native.dart", directory.path(), invitation.ticket())
            .env("SAILRY_SESSION", session.id.to_string())
            .env("SAILRY_NATIVE_API", name)
            .spawn()
            .unwrap();
        let status = super::wait(child);
        runtime.block_on(target.shutdown()).unwrap();
        if !status.success() || server.requests.lock().unwrap().len() != 3 {
            let requests = server.requests.lock().unwrap();
            eprintln!(
                "native fixture: api={name}, received_requests={}",
                requests.len()
            );
            for (index, request) in requests.iter().enumerate() {
                let body = &request.body;
                let messages = body["messages"].as_array();
                let blocks = messages
                    .into_iter()
                    .flatten()
                    .filter_map(|message| message["content"].as_array())
                    .flatten()
                    .filter_map(|block| block["type"].as_str())
                    .collect::<Vec<_>>();
                eprintln!(
                    "native fixture request {index}: summary={}, max_tokens={:?}, thinking_budget={:?}, messages={}, tools={}, blocks={blocks:?}",
                    body.to_string().contains("Summarize this conversation"),
                    body["max_tokens"].as_u64(),
                    body["thinking"]["budget_tokens"].as_u64(),
                    messages.map_or(0, Vec::len),
                    body["tools"].as_array().map_or(0, Vec::len),
                );
            }
        }
        assert!(status.success(), "native fixture failed: {name}");
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        let encoded = base64::engine::general_purpose::STANDARD
            .encode(include_bytes!("../../../../tests/fixtures/document.pdf"));
        for request in requests.iter() {
            for (name, mime, bytes) in [
                (
                    "audio.wav.bin",
                    "audio/wav",
                    include_bytes!("../../../../tests/fixtures/audio.wav").as_slice(),
                ),
                (
                    "video.mp4.bin",
                    "video/mp4",
                    include_bytes!("../../../../tests/fixtures/video.mp4").as_slice(),
                ),
            ] {
                let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                if api == ModelApi::Gemini {
                    assert!(
                        request.body["contents"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .flat_map(|message| message["parts"].as_array().unwrap())
                            .all(|part| part["inline_data"]["mime_type"] != mime)
                    );
                }
                let serialized = request.body.to_string();
                assert!(!serialized.contains(&encoded));
                assert!(serialized.contains(name));
                assert!(
                    serialized.contains(r#"Contents not included; use read_office with source {\"kind\":\"attachment\",\"value\":\"<id>\"} for PDF, Office documents and spreadsheets; use an attachment-capable tool for other files"#)
                );
            }
            let messages = if api == ModelApi::Anthropic {
                "messages"
            } else {
                "contents"
            };
            let parts = if api == ModelApi::Anthropic {
                "content"
            } else {
                "parts"
            };
            let pdf = request.body[messages]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|message| message[parts].as_array())
                .flatten()
                .find(|part| {
                    part["type"] == "document"
                        || part["inline_data"]["mime_type"] == "application/pdf"
                })
                .expect("native PDF input expected");
            if api == ModelApi::Anthropic {
                assert_eq!(request.body["model"], model);
                assert_eq!(request.body["max_tokens"], 4096);
                assert_eq!(pdf["source"]["type"], "base64");
                assert_eq!(pdf["source"]["media_type"], "application/pdf");
                assert_eq!(pdf["source"]["data"], encoded);
                assert_eq!(
                    request.body["thinking"],
                    serde_json::json!({"type":"enabled","budget_tokens":1024})
                );
                assert!(request.body.get("output_config").is_none());
            } else {
                assert!(
                    request
                        .path
                        .contains(&format!("/models/{model}:streamGenerateContent"))
                );
                assert_eq!(request.body["generationConfig"]["maxOutputTokens"], 4096);
                assert_eq!(pdf["inline_data"]["data"], encoded);
                assert_eq!(
                    request.body["generationConfig"]["thinkingConfig"],
                    serde_json::json!({"thinkingBudget":1024,"includeThoughts":true})
                );
            }
        }
        let header = if api == ModelApi::Anthropic {
            "x-api-key"
        } else {
            "x-goog-api-key"
        };
        assert_eq!(
            requests[0].headers[header],
            "isolated-ffi-configuration-credential"
        );
        assert!(requests[1].body.to_string().contains("工具内容 🙂"));
    }
}
