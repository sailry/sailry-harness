use super::*;
use base64::Engine;

#[tokio::test]
async fn cloud_images_survive_restart() {
    for api in [
        ModelApi::DeepSeek,
        ModelApi::AzureOpenAi,
        ModelApi::AzureAi,
        ModelApi::Bedrock,
        ModelApi::Vertex,
    ] {
        let mut fixture = Fixture::start().await;
        let server = provider::Server::start(api, provider::Reply::Text).await;
        let bytes = png();
        let mut records = Vec::new();
        for client in fixture.clients() {
            let (session, _) = session(&client, fixture.worktree, &server.endpoint, api).await;
            let image = upload(&client, fixture.worktree, "image.png", "image/png", &bytes).await;
            let turn = queue(&client, &session, input("Inspect the image", &[&image])).await;
            let history = run(&client, &turn).await;
            assert_eq!(
                history.runs[0].status,
                Status::Completed,
                "{:?}",
                history.runs
            );
            records.push((session, history));
        }
        let profile = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        fixture.node = Node::start(profile).await.unwrap();
        for (client, (session, history)) in fixture.clients().into_iter().zip(records) {
            assert_eq!(page(&client, session.id).await, history);
            let turn = queue(&client, &session, "Continue".into()).await;
            assert_eq!(
                run(&client, &turn).await.runs.last().unwrap().status,
                Status::Completed
            );
        }
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 4);
        for request in requests {
            if api == ModelApi::Vertex {
                let parts = request.body["contents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .flat_map(|message| message["parts"].as_array().unwrap())
                    .collect::<Vec<_>>();
                let image = parts
                    .iter()
                    .find(|part| part["inlineData"]["mimeType"] == "image/png")
                    .unwrap();
                assert_eq!(
                    image["inlineData"]["data"],
                    base64::engine::general_purpose::STANDARD.encode(&bytes)
                );
                continue;
            }
            let parts = request.body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|message| message["role"] == "user")
                .filter_map(|message| message["content"].as_array())
                .flatten()
                .collect::<Vec<_>>();
            if api == ModelApi::Bedrock {
                let image = parts.iter().find(|part| part["image"].is_object()).unwrap();
                assert_eq!(image["image"]["format"], "png");
                assert_eq!(
                    image["image"]["source"]["bytes"],
                    base64::engine::general_purpose::STANDARD.encode(&bytes)
                );
                continue;
            }
            let image = parts
                .iter()
                .find(|part| part["type"] == "image_url")
                .unwrap();
            assert_eq!(
                image["image_url"]["url"],
                format!(
                    "data:image/png;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(&bytes)
                )
            );
            assert!(parts.iter().any(|part| part["text"] == "Inspect the image"));
            assert!(!request.body.to_string().contains("sailry-attachment://"));
        }
        fixture.close().await;
    }
}

#[tokio::test]
async fn restores_native_inputs() {
    let bytes = png();
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let pdf_bytes = pdf();
    let pdf_encoded = base64::engine::general_purpose::STANDARD.encode(&pdf_bytes);
    for api in [ModelApi::Anthropic, ModelApi::Gemini] {
        let mut fixture = Fixture::start().await;
        let server = provider::Server::start(api, provider::Reply::Text).await;
        let mut records = Vec::new();
        for client in fixture.clients() {
            let (session, _) = session(&client, fixture.worktree, &server.endpoint, api).await;
            let image = upload(&client, fixture.worktree, "图片.png", "image/png", &bytes).await;
            let document = upload(
                &client,
                fixture.worktree,
                "内容.txt",
                "text/plain",
                "完整内容 🙂".as_bytes(),
            )
            .await;
            let pdf = upload(
                &client,
                fixture.worktree,
                "报告.bin",
                "application/octet-stream",
                &pdf_bytes,
            )
            .await;
            let mut attachments = vec![image, document, pdf.clone()];
            if api == ModelApi::Gemini {
                for (name, _, bytes) in media() {
                    attachments
                        .push(upload(&client, fixture.worktree, name, "text/plain", bytes).await);
                }
            }
            let turn = queue(
                &client,
                &session,
                input("Inspect", &attachments.iter().collect::<Vec<_>>()),
            )
            .await;
            let history = run(&client, &turn).await;
            assert_eq!(
                history.runs[0].status,
                Status::Completed,
                "{:?}",
                history.runs
            );
            assert!(
                history
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| *part == Part::Attachment(pdf.clone()))
            );
            records.push((session, history));
        }
        let profile = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        fixture.node = Node::start(profile).await.unwrap();
        for (client, (session, history)) in fixture.clients().into_iter().zip(records) {
            assert_eq!(page(&client, session.id).await, history);
            let turn = queue(&client, &session, "Continue".into()).await;
            assert_eq!(
                run(&client, &turn).await.runs.last().unwrap().status,
                Status::Completed
            );
        }
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 4);
        for request in requests {
            let body = request.body;
            assert!(
                body.to_string()
                    .contains("Attachment: 内容.txt\\n完整内容 🙂")
            );
            assert!(!body.to_string().contains("sailry-attachment://"));
            let parts: Vec<_> = match api {
                ModelApi::Anthropic => body["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|message| message["role"] == "user")
                    .flat_map(|message| message["content"].as_array().unwrap())
                    .collect(),
                ModelApi::Gemini => body["contents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|message| message["role"] == "user")
                    .flat_map(|message| message["parts"].as_array().unwrap())
                    .collect(),
                _ => unreachable!(),
            };
            let image = parts
                .iter()
                .find(|part| part["type"] == "image" || !part["inlineData"].is_null())
                .unwrap();
            match api {
                ModelApi::Anthropic => {
                    assert_eq!(image["source"]["media_type"], "image/png");
                    assert_eq!(image["source"]["data"], encoded);
                    let pdf = parts
                        .iter()
                        .find(|part| part["type"] == "document")
                        .unwrap();
                    assert_eq!(pdf["source"]["type"], "base64");
                    assert_eq!(pdf["source"]["media_type"], "application/pdf");
                    assert_eq!(pdf["source"]["data"], pdf_encoded);
                    assert!(body["thinking"].is_null());
                    assert!(body["tools"].as_array().is_none_or(Vec::is_empty));
                }
                ModelApi::Gemini => {
                    assert_eq!(image["inlineData"]["mimeType"], "image/png");
                    assert_eq!(image["inlineData"]["data"], encoded);
                    let pdf = parts
                        .iter()
                        .find(|part| part["inlineData"]["mimeType"] == "application/pdf")
                        .unwrap();
                    assert_eq!(pdf["inlineData"]["data"], pdf_encoded);
                    for (_, mime, bytes) in media() {
                        let part = parts
                            .iter()
                            .find(|part| part["inlineData"]["mimeType"] == mime)
                            .unwrap();
                        assert_eq!(
                            part["inlineData"]["data"],
                            base64::engine::general_purpose::STANDARD.encode(bytes)
                        );
                    }
                    assert!(body["generationConfig"]["thinkingConfig"].is_null());
                    assert!(body["tools"].as_array().is_none_or(Vec::is_empty));
                }
                _ => unreachable!(),
            }
        }
        fixture.close().await;
    }
}

pub(super) fn media() -> [(&'static str, &'static str, &'static [u8]); 2] {
    [
        (
            "audio.txt",
            "audio/wav",
            include_bytes!("../../../../../tests/fixtures/audio.wav"),
        ),
        (
            "video.txt",
            "video/mp4",
            include_bytes!("../../../../../tests/fixtures/video.mp4"),
        ),
    ]
}
