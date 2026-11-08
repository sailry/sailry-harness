use super::*;

#[tokio::test]
async fn preserves_document_names_and_order() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let reply = if api == ModelApi::Responses {
            fixtures::responses(false)
        } else {
            fixtures::chat()
        };
        let server = Server::start(vec![reply]).await;
        let provider = provider(api, server.endpoint.clone());
        let model = native(&provider, "fixture", "key", HeaderMap::new());
        let mut input = request("fixture");
        input.messages = vec![Message::user().with_text("Before")];
        for name in [Some("report.pdf"), Some("报告.bin"), None] {
            input.messages[0].content.extend([
                Content::Document {
                    media_type: "application/pdf".into(),
                    name: name.map(str::to_owned),
                    data_base64: "cGRm".into(),
                },
                Content::Text {
                    text: "Between documents".into(),
                },
            ]);
        }
        collect(&model, input, CancellationToken::new())
            .await
            .unwrap();
        let captured = server.finish().await;
        let body = &captured[0].body;
        let messages = &body[if api == ModelApi::Responses {
            "input"
        } else {
            "messages"
        }];
        let content = messages
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "user")
            .flat_map(|message| message["content"].as_array().unwrap())
            .cloned()
            .collect::<Vec<_>>();
        let text_kind = if api == ModelApi::Responses {
            "input_text"
        } else {
            "text"
        };
        let mut expected = vec![json!({"type":text_kind,"text":"Before"})];
        for name in ["report.pdf", "报告.bin", "document.pdf"] {
            let file = json!({
                "filename":name,"file_data":"data:application/pdf;base64,cGRm",
            });
            expected.push(if api == ModelApi::Responses {
                let mut part = file;
                part["type"] = "input_file".into();
                part
            } else {
                json!({"type":"file","file":file})
            });
            expected.push(json!({"type":text_kind,"text":"Between documents"}));
        }
        assert_eq!(content, expected, "{api:?}");
        assert!(!body.to_string().contains("__sailry_tool_media"));
        assert!(!body.to_string().contains("sailry-media:"));
    }
}

#[tokio::test]
async fn preserves_explicit_chat_images() {
    for (api, name) in [
        (ModelApi::ChatCompletions, "fixture"),
        (ModelApi::ChatCompletions, "gpt-3.5-turbo"),
        (ModelApi::DeepSeek, "deepseek-chat"),
    ] {
        for caption in [false, true] {
            let server = Server::start(vec![fixtures::chat()]).await;
            let provider = provider(api, server.endpoint.clone());
            let model = native(&provider, name, "key", HeaderMap::new());
            let mut input = request(name);
            input.messages = vec![Message::user().with_content(Content::Image {
                media_type: "image/png".into(),
                data: Image::Inline {
                    data_base64: "aW1hZ2U=".into(),
                },
                annotations: None,
            })];
            let mut expected = vec![json!({
                "type":"image_url","image_url":{
                    "url":"data:image/png;base64,aW1hZ2U=","detail":"auto",
                },
            })];
            if caption {
                input.messages[0].content.extend([
                    Content::Text {
                        text: "Inspect both images".into(),
                    },
                    Content::Image {
                        media_type: "image/jpeg".into(),
                        data: Image::Url {
                            url: "https://example.com/image.jpg".into(),
                        },
                        annotations: None,
                    },
                ]);
                expected.extend([
                    json!({"type":"text","text":"Inspect both images"}),
                    json!({
                        "type":"image_url","image_url":{
                            "url":"https://example.com/image.jpg","detail":"auto",
                        },
                    }),
                ]);
            }
            collect(&model, input, CancellationToken::new())
                .await
                .unwrap();
            let captured = server.finish().await;
            let body = &captured[0].body;
            let user = body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|message| message["role"] == "user")
                .unwrap();
            assert_eq!(user["content"], json!(expected), "{api:?} {name}");
            assert!(!body.to_string().contains("__sailry_tool_media"));
            assert!(!body.to_string().contains("sailry-media:"));
        }
    }
}
