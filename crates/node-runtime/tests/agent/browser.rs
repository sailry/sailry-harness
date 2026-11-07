use super::*;
use serde_json::json;

#[tokio::test]
async fn routes_through_controller() {
    for remote in [false, true] {
        for mode in [WorkMode::Code, WorkMode::Plan] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("project");
            std::fs::create_dir(&root).unwrap();
            let node = Node::start_empty(directory.path().join("node"))
                .await
                .unwrap();
            let controller =
                Link::controller(directory.path().join("controller"), NetworkScope::default())
                    .await
                    .unwrap();
            let address = controller
                .handle()
                .pair(node.link().invite().unwrap().ticket())
                .await
                .unwrap();
            let transport: Arc<dyn Transport> = if remote {
                controller.handle().remote(address)
            } else {
                node.local()
            };
            let client = Arc::new(Client::new(transport));
            let mut subscription = client.subscribe_browser().await.unwrap();
            let mut calls: Vec<(String, serde_json::Value)> = vec![
                (plugin_tool("browser", "browser_tabs"), json!({})),
                (
                    plugin_tool("browser", "browser_navigate"),
                    json!({"url":"https://example.com"}),
                ),
                (plugin_tool("browser", "browser_read"), json!({"tab":0})),
                (
                    plugin_tool("browser", "browser_click"),
                    json!({"tab":0,"snapshot":"fixture","element":1}),
                ),
                (
                    plugin_tool("browser", "browser_input"),
                    json!({"tab":0,"snapshot":"fixture","element":2,"text":"hello"}),
                ),
                (
                    plugin_tool("browser", "browser_scroll"),
                    json!({"tab":0,"direction":"down"}),
                ),
                (plugin_tool("browser", "browser_back"), json!({"tab":0})),
                (plugin_tool("browser", "browser_forward"), json!({"tab":0})),
                (plugin_tool("browser", "browser_refresh"), json!({"tab":0})),
                (
                    plugin_tool("browser", "browser_open"),
                    json!({"url":"https://example.com"}),
                ),
                (plugin_tool("browser", "browser_focus"), json!({"tab":0})),
                (plugin_tool("browser", "browser_close"), json!({"tab":0})),
                (
                    plugin_tool("browser", "browser_select"),
                    json!({"tab":0,"snapshot":"fixture","element":1,"value":"b"}),
                ),
                (
                    plugin_tool("browser", "browser_hover"),
                    json!({"tab":0,"snapshot":"fixture","element":1}),
                ),
                (
                    plugin_tool("browser", "browser_key"),
                    json!({"tab":0,"snapshot":"fixture","element":1,"key":"Enter"}),
                ),
                (
                    plugin_tool("browser", "browser_frame"),
                    json!({"tab":0,"snapshot":"fixture","element":1}),
                ),
                (
                    plugin_tool("browser", "browser_wait"),
                    json!({"tab":0,"condition":{"kind":"text","text":"Ready"},"timeout_ms":500}),
                ),
            ];
            let mutating = [
                "browser_click",
                "browser_input",
                "browser_select",
                "browser_hover",
                "browser_key",
                "browser_close",
            ];
            if mode == WorkMode::Plan {
                calls.retain(|(name, _)| {
                    !mutating
                        .iter()
                        .any(|tool| *name == plugin_tool("browser", tool))
                });
            }
            let mut discovered: Vec<_> = calls.iter().map(|(name, _)| name.clone()).collect();
            if mode == WorkMode::Plan {
                discovered.extend(mutating.map(|name| plugin_tool("browser", name)));
            }
            let mut script: Vec<_> = discovered
                .iter()
                .map(|name| ("tool_search".into(), json!({"query":name,"limit":1})))
                .collect();
            script.extend(calls.clone());
            let count = calls.len();
            let server = Server::scripted_tools(script.clone()).await;
            let session =
                super::approvals::prepare_packages(&client, &server, &root, &["browser"]).await;
            let mut config = session.config.clone();
            config.permission = Permission::Full;
            config.mode = mode;
            client
                .execute(client.prepare(Command::SetSessionConfig {
                    session: session.id,
                    expected_revision: session.revision,
                    config,
                }))
                .await
                .unwrap();
            let responder = client.clone();
            let browser = tokio::spawn(async move {
                for _ in 0..count {
                    let Update::BrowserCall(call) = subscription.next().await.unwrap() else {
                        panic!("browser call expected")
                    };
                    assert_eq!(call.session, session.id);
                    assert!(matches!(
                        responder
                            .execute(responder.prepare(Command::CompleteBrowser {
                                id: call.id,
                                result: Ok(
                                    json!({"text":"Browser fixture","tab":0,"snapshot":"fixture"})
                                )
                            }))
                            .await
                            .unwrap(),
                        Output::BrowserCompleted
                    ));
                }
                subscription
            });
            let Output::QueuedTurn(turn) = client
                .execute(client.prepare(Command::SubmitTurn {
                    session: session.id,
                    expected_revision: 2,
                    message: "Use the built-in browser".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            let page = if mode == WorkMode::Plan {
                questions::finish_plan(&client, session.id, turn.id).await
            } else {
                finished(&client, session.id, turn.id).await
            };
            super::evaluation::trajectory(&client, &page, &script).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                page.runs
            );
            let mut subscription = tokio::time::timeout(Duration::from_secs(2), browser)
                .await
                .unwrap_or_else(|error| {
                    panic!(
                        "browser responder failed in {mode:?} (remote={remote}): {error}; results={:?}",
                        page.entries
                    )
                })
                .unwrap();
            let requests = server.requests.lock().unwrap().clone();
            let initial = requests[0]["tools"].as_array().unwrap();
            assert!(
                initial
                    .iter()
                    .any(|tool| tool["function"]["name"] == "tool_search")
            );
            for (index, name) in discovered.iter().enumerate() {
                assert!(
                    initial
                        .iter()
                        .all(|tool| tool["function"]["name"] != exposed(name))
                );
                assert!(
                    requests[index + 1]["tools"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|tool| tool["function"]["name"] == exposed(name))
                );
            }
            let tools = requests[discovered.len()]["tools"].as_array().unwrap();
            assert!(
                tools
                    .iter()
                    .all(|tool| tool["function"]["name"] != "browser_screenshot")
            );
            for (name, _) in calls {
                assert!(
                    tools
                        .iter()
                        .any(|tool| tool["function"]["name"] == exposed(&name))
                );
            }
            if mode == WorkMode::Plan {
                for name in mutating {
                    assert!(tools.iter().any(|tool| tool["function"]["name"] == name));
                }
                assert!(page.approvals.is_empty());
            } else {
                assert_eq!(page.approvals.len(), mutating.len());
            }
            let results: Vec<_> = tool_details(&client, &page)
                .await
                .into_iter()
                .filter(|(name, _)| name != "plan_review" && name != "tool_search")
                .map(|(_, detail)| detail.result.unwrap())
                .collect();
            assert_eq!(results.len(), count);
            assert!(
                results
                    .iter()
                    .all(|result| result["text"] == "Browser fixture")
            );
            if mode == WorkMode::Plan {
                server.replace_tools(vec![(
                    plugin_tool("browser", "browser_click"),
                    json!({"tab":0,"snapshot":"fixture","element":1}),
                )]);
                let Output::QueuedTurn(denied) = client
                    .execute(client.prepare(Command::SubmitTurn {
                        session: session.id,
                        expected_revision: 2,
                        message: "Keep the planning browser read-only".into(),
                    }))
                    .await
                    .unwrap()
                else {
                    panic!("denied browser turn expected")
                };
                let denied_page = finished(&client, session.id, denied.id).await;
                planning_denial(&client, &denied_page, denied.id).await;
                assert!(
                    tokio::time::timeout(Duration::from_millis(20), subscription.next())
                        .await
                        .is_err()
                );
            }
            drop(subscription);
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}

fn encoded(format: image::ImageFormat) -> (Vec<u8>, String) {
    use base64::Engine;
    let pixels = image::RgbImage::from_pixel(2, 2, image::Rgb([32, 96, 160]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    pixels.write_to(&mut bytes, format).unwrap();
    let bytes = bytes.into_inner();
    let text = base64::engine::general_purpose::STANDARD.encode(&bytes);
    (bytes, text)
}

/// Image blocks in an outgoing Chat Completions request.
fn uploaded(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Object(fields) => {
            usize::from(fields.get("type").and_then(serde_json::Value::as_str) == Some("image_url"))
                + fields.values().map(uploaded).sum::<usize>()
        }
        serde_json::Value::Array(values) => values.iter().map(uploaded).sum(),
        _ => 0,
    }
}

async fn capture_session(
    client: &Client,
    server: &Server,
    root: &std::path::Path,
    vision: bool,
) -> Session {
    super::approvals::install(client, &["browser"]).await;
    let (session, mut provider) = configured_session(client, &server.endpoint, root).await;
    provider.default_model = "fixture-a".into();
    provider.models = vec![Model {
        id: "fixture-a".into(),
        display_name: None,
        context: 128000,
        output: 2048,
        vision,
        tools: true,
        reasoning: false,
        generates: vec![],
        efforts: Vec::new(),
        capabilities: None,
        custom_efforts: false,
        compaction_tokens: None,
        default_effort: sailry_protocol::Effort::Default,
    }];
    client
        .execute(client.prepare(Command::PutProvider {
            provider,
            expected_revision: 1,
        }))
        .await
        .unwrap();
    session
}

/// The first tool result image and the reference the model received for it.
fn captured(page: &Page) -> (sailry_protocol::conversation::images::Reference, Image) {
    page.entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .find_map(|part| match part {
            Part::ToolResult {
                id: Some(id),
                images: found,
                ..
            } if !found.is_empty() => Some((
                sailry_protocol::conversation::images::Reference {
                    call: id.clone(),
                    index: found[0].index,
                },
                found[0].clone(),
            )),
            _ => None,
        })
        .expect("captured tool image")
}

#[tokio::test]
async fn shows_captures_to_models() {
    use sailry_protocol::browser::{Action, Format, Scale, Target};
    let (png, encoded) = encoded(image::ImageFormat::Png);
    for remote in [false, true] {
        for vision in [true, false] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("project");
            std::fs::create_dir(&root).unwrap();
            let node = Node::start_empty(directory.path().join("node"))
                .await
                .unwrap();
            let controller =
                Link::controller(directory.path().join("controller"), NetworkScope::default())
                    .await
                    .unwrap();
            let address = controller
                .handle()
                .pair(node.link().invite().unwrap().ticket())
                .await
                .unwrap();
            let client = Client::new(if remote {
                controller.handle().remote(address)
            } else {
                node.local()
            });
            let mut subscription = client.subscribe_browser().await.unwrap();
            let name = plugin_tool("browser", "browser_screenshot");
            let server = Server::tools(vec![(
                name.clone(),
                json!({"tab":7,"target":{"kind":"selector","selector":"#save"},"scale":"device","format":"png"}),
            )])
            .await;
            let session = capture_session(&client, &server, &root, vision).await;
            let turn = approvals::submit(&client, session.id).await;
            let Update::BrowserCall(call) =
                tokio::time::timeout(Duration::from_secs(5), subscription.next())
                    .await
                    .unwrap()
                    .unwrap()
            else {
                panic!("browser call expected")
            };
            assert_eq!(
                call.action,
                Action::Screenshot {
                    tab: 7,
                    target: Some(Target::Selector {
                        selector: "#save".into()
                    }),
                    scale: Scale::Device,
                    format: Format::Png,
                    save: false,
                }
            );
            // Synthetic pixels exercise publication without calling a screen capture API.
            client
                .execute(client.prepare(Command::CompleteBrowser {
                    id: call.id,
                    result: Ok(json!({
                        "tab":7,"mime_type":"image/png","base64_image":encoded,
                        "bounds":{"x":4,"y":8,"width":2,"height":2},"tag":"button","text":"Save",
                        "path":"../controller/escaped.png"
                    })),
                }))
                .await
                .unwrap();
            let page = finished(&client, session.id, turn).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
            assert!(page.approvals.is_empty());
            let (reference, image) = captured(&page);
            assert_eq!(
                sailry_client::conversation::images::resolve(&page, &reference),
                Some(&image)
            );
            assert_eq!(image.attachment.spec.media_type, "image/png");
            let results: Vec<_> = tool_details(&client, &page)
                .await
                .into_iter()
                .filter(|(tool, _)| *tool == name)
                .map(|(_, detail)| detail.result.unwrap())
                .collect();
            assert_eq!(results.len(), 1);
            assert_eq!(
                results[0]["response"],
                json!({
                    "tab":7,"bounds":{"x":4,"y":8,"width":2,"height":2},"tag":"button","text":"Save",
                    "mime_type":"image/png","width":2,"height":2,"images":[reference.to_string()]
                })
            );
            assert!(!root.join("assets").exists());
            assert!(!directory.path().join("controller/escaped.png").exists());
            let Output::AttachmentDownload(download) = client
                .execute(client.prepare(Command::DownloadImage {
                    session: session.id,
                    image: image.clone(),
                }))
                .await
                .unwrap()
            else {
                panic!("image download expected")
            };
            let mut downloaded = Vec::new();
            client
                .download_attachment(
                    &download,
                    &mut downloaded,
                    sailry_link::CancellationToken::new(),
                    |_| {},
                )
                .await
                .unwrap();
            assert_eq!(downloaded, png);
            let requests = server.requests.lock().unwrap().clone();
            let last = requests.last().unwrap();
            let result = last["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|message| message["role"] == "tool")
                .expect("canonical tool result must reach the next model request")["content"]
                .to_string();
            for fact in [reference.to_string().as_str(), "button", "Save"] {
                assert!(result.contains(fact), "{fact}: {result}");
            }
            let text = last.to_string();
            if vision {
                assert_eq!(uploaded(last), 1);
                assert!(text.contains("data:image/png;base64,"));
            } else {
                assert_eq!(uploaded(last), 0);
                assert!(text.contains("does not support images"));
                assert!(!text.contains("data:image/"));
            }
            drop(subscription);
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn saves_confirmed_captures() {
    use sailry_protocol::browser::{Action, Format, Scale};
    let (jpeg, encoded) = encoded(image::ImageFormat::Jpeg);
    for remote in [false, true] {
        for mode in [WorkMode::Code, WorkMode::Plan] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("project");
            std::fs::create_dir(&root).unwrap();
            let node = Node::start_empty(directory.path().join("node"))
                .await
                .unwrap();
            let controller =
                Link::controller(directory.path().join("controller"), NetworkScope::default())
                    .await
                    .unwrap();
            let address = controller
                .handle()
                .pair(node.link().invite().unwrap().ticket())
                .await
                .unwrap();
            let client = Client::new(if remote {
                controller.handle().remote(address)
            } else {
                node.local()
            });
            let mut subscription = client.subscribe_browser().await.unwrap();
            let name = plugin_tool("browser", "browser_screenshot");
            let server = Server::tools(vec![(name.clone(), json!({"tab":7,"save":true}))]).await;
            let session = approvals::prepare_packages(&client, &server, &root, &["browser"]).await;
            let mut revision = session.revision;
            if mode == WorkMode::Plan {
                let mut config = session.config.clone();
                config.mode = mode;
                client
                    .execute(client.prepare(Command::SetSessionConfig {
                        session: session.id,
                        expected_revision: session.revision,
                        config,
                    }))
                    .await
                    .unwrap();
                revision += 1;
            }
            let Output::QueuedTurn(turn) = client
                .execute(client.prepare(Command::SubmitTurn {
                    session: session.id,
                    expected_revision: revision,
                    message: "Save a browser capture".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            let details = |page: Page| {
                let client = &client;
                let name = name.clone();
                async move {
                    tool_details(client, &page)
                        .await
                        .into_iter()
                        .filter(|(tool, _)| *tool == name)
                        .map(|(_, detail)| detail)
                        .collect::<Vec<_>>()
                }
            };
            if mode == WorkMode::Plan {
                let page = questions::finish_plan(&client, session.id, turn.id).await;
                assert_eq!(page.runs.last().unwrap().status, Status::Completed);
                assert!(page.approvals.is_empty());
                assert!(
                    tokio::time::timeout(Duration::from_millis(20), subscription.next())
                        .await
                        .is_err()
                );
                let results = details(page).await;
                assert_eq!(
                    results[0].result.as_ref().unwrap()["error"]["message"],
                    "planning turns cannot save browser captures"
                );
                assert!(!root.join("assets").exists());
            } else {
                let (_, approval) = approvals::pending(&client, session.id).await;
                assert!(
                    tokio::time::timeout(Duration::from_millis(20), subscription.next())
                        .await
                        .is_err()
                );
                client
                    .execute(client.prepare(Command::ResolveApproval {
                        session: session.id,
                        approval: approval.id,
                        decision: Decision::Approve,
                    }))
                    .await
                    .unwrap();
                let Update::BrowserCall(call) =
                    tokio::time::timeout(Duration::from_secs(5), subscription.next())
                        .await
                        .unwrap()
                        .unwrap()
                else {
                    panic!("browser call expected")
                };
                assert_eq!(
                    call.action,
                    Action::Screenshot {
                        tab: 7,
                        target: None,
                        scale: Scale::Css,
                        format: Format::Jpeg,
                        save: true,
                    }
                );
                client
                    .execute(client.prepare(Command::CompleteBrowser {
                        id: call.id,
                        result: Ok(json!({
                            "tab":7,"mime_type":"image/jpeg","width":2,"height":2,
                            "base64_image":encoded,"path":"../controller/escaped.jpg"
                        })),
                    }))
                    .await
                    .unwrap();
                let page = finished(&client, session.id, turn.id).await;
                assert_eq!(page.runs.last().unwrap().status, Status::Completed);
                assert_eq!(page.approvals.len(), 1);
                let (reference, _) = captured(&page);
                let path = format!("assets/generated/browser-{}.jpg", call.id);
                let results = details(page).await;
                assert_eq!(results[0].images.len(), 1);
                assert_eq!(
                    results[0].result.as_ref().unwrap()["response"],
                    json!({
                        "tab":7,"mime_type":"image/jpeg","width":2,"height":2,
                        "path":path,"images":[reference.to_string()]
                    })
                );
                assert!(!directory.path().join("controller/escaped.jpg").exists());
                assert!(
                    root.join(&path)
                        .canonicalize()
                        .unwrap()
                        .starts_with(root.canonicalize().unwrap())
                );
                let Output::FileDownload(download) = client
                    .execute(client.prepare(Command::DownloadFile {
                        worktree: session.worktree,
                        path: path.clone(),
                    }))
                    .await
                    .unwrap()
                else {
                    panic!("artifact download expected")
                };
                let mut downloaded = Vec::new();
                client
                    .download(
                        &download,
                        &mut downloaded,
                        sailry_link::CancellationToken::new(),
                        |_| {},
                    )
                    .await
                    .unwrap();
                assert_eq!(downloaded, jpeg);
            }
            drop(subscription);
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn retains_recent_captures() {
    let (_, encoded) = encoded(image::ImageFormat::Png);
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Arc::new(Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        }));
        let mut subscription = client.subscribe_browser().await.unwrap();
        let name = plugin_tool("browser", "browser_screenshot");
        let server = Server::tools(
            (0..10)
                .map(|_| (name.clone(), json!({"tab":0,"format":"png"})))
                .collect(),
        )
        .await;
        let session = capture_session(&client, &server, &root, true).await;
        let responder = client.clone();
        let image = encoded.clone();
        let browser = tokio::spawn(async move {
            for _ in 0..10 {
                let Update::BrowserCall(call) = subscription.next().await.unwrap() else {
                    panic!("browser call expected")
                };
                responder
                    .execute(responder.prepare(Command::CompleteBrowser {
                        id: call.id,
                        result: Ok(json!({"tab":0,"base64_image":image})),
                    }))
                    .await
                    .unwrap();
            }
            subscription
        });
        let turn = approvals::submit(&client, session.id).await;
        let page = finished(&client, session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let subscription = tokio::time::timeout(Duration::from_secs(2), browser)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            tool_details(&client, &page)
                .await
                .iter()
                .filter(|(tool, _)| *tool == name)
                .map(|(_, detail)| detail.images.len())
                .sum::<usize>(),
            10
        );
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 11);
        assert_eq!(uploaded(&requests[9]), 9);
        assert_eq!(uploaded(&requests[10]), 5);
        assert!(requests[10].to_string().contains("omitted_screenshots"));
        drop(subscription);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_denied_clicks() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let mut subscription = client.subscribe_browser().await.unwrap();
        let server = Server::tools(vec![(
            plugin_tool("browser", "browser_click"),
            json!({"tab":0,"snapshot":"fixture","element":1}),
        )])
        .await;
        let session =
            super::approvals::prepare_packages(&client, &server, &root, &["browser"]).await;
        let turn = super::approvals::submit(&client, session.id).await;
        let (_, approval) = super::approvals::pending(&client, session.id).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(20), subscription.next())
                .await
                .is_err()
        );
        client
            .execute(client.prepare(Command::ResolveApproval {
                session: session.id,
                approval: approval.id,
                decision: Decision::Deny,
            }))
            .await
            .unwrap();
        let page = finished(&client, session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), subscription.next())
                .await
                .is_err()
        );
        drop(subscription);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn discards_cancelled_completion() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let mut subscription = client.subscribe_browser().await.unwrap();
        let server = Server::tools(vec![(
            plugin_tool("browser", "browser_wait"),
            json!({"tab":0,"condition":{"kind":"text","text":"Pending"},"timeout_ms":20000}),
        )])
        .await;
        let session =
            super::approvals::prepare_packages(&client, &server, &root, &["browser"]).await;
        let turn = super::approvals::submit(&client, session.id).await;
        let Update::BrowserCall(call) =
            tokio::time::timeout(Duration::from_secs(5), subscription.next())
                .await
                .unwrap()
                .unwrap()
        else {
            panic!("browser call expected")
        };
        client
            .execute(client.prepare(Command::StopTurn { turn }))
            .await
            .unwrap();
        assert_eq!(
            finished(&client, session.id, turn)
                .await
                .runs
                .last()
                .unwrap()
                .status,
            Status::Cancelled
        );
        assert!(
            client
                .execute(client.prepare(Command::CompleteBrowser {
                    id: call.id,
                    result: Ok(json!({"ready":true}))
                }))
                .await
                .is_err()
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), subscription.next())
                .await
                .is_err()
        );
        drop(subscription);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn omits_disabled_tools() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Arc::new(Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        }));
        let _subscription = client.subscribe_browser().await.unwrap();
        let server = Server::markdown("Ready".into()).await;
        let session =
            super::approvals::prepare_packages(&client, &server, &root, &["browser"]).await;

        client
            .execute(client.prepare(Command::SetPluginEnabled {
                name: "browser".into(),
                expected_revision: 1,
                enabled: false,
            }))
            .await
            .unwrap();
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Continue".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        finished(&client, session.id, turn.id).await;
        {
            let requests = server.requests.lock().unwrap();
            assert!(!requests.is_empty());
            for request in requests.iter() {
                let tools = request["tools"].as_array().unwrap();
                assert!(!tools.is_empty());
                assert!(tools.iter().all(|tool| {
                    let name = tool["function"]["name"].as_str().unwrap_or_default();
                    !name.starts_with(&plugin_tool("browser", "")) && !name.starts_with("browser_")
                }));
            }
        }
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn rechecks_enabled_state() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let mut subscription = client.subscribe_browser().await.unwrap();
        let server = Server::tools(vec![(
            plugin_tool("browser", "browser_click"),
            json!({"tab":0,"snapshot":"fixture","element":1}),
        )])
        .await;
        let session =
            super::approvals::prepare_packages(&client, &server, &root, &["browser"]).await;
        let turn = super::approvals::submit(&client, session.id).await;
        let (_, approval) = super::approvals::pending(&client, session.id).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(20), subscription.next())
                .await
                .is_err()
        );
        client
            .execute(client.prepare(Command::SetPluginEnabled {
                name: "browser".into(),
                expected_revision: 1,
                enabled: false,
            }))
            .await
            .unwrap();
        client
            .execute(client.prepare(Command::ResolveApproval {
                session: session.id,
                approval: approval.id,
                decision: Decision::Approve,
            }))
            .await
            .unwrap();
        let page = finished(&client, session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), subscription.next())
                .await
                .is_err()
        );
        drop(subscription);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
