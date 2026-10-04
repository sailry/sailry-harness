use super::*;
use serde_json::json;

#[tokio::test]
async fn routes_through_controller() {
    for remote in [false, true] {
        for mode in [WorkMode::Code, WorkMode::Plan] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("project");
            std::fs::create_dir(&root).unwrap();
            let node = Node::start(directory.path().join("node")).await.unwrap();
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
            let count = calls.len();
            let server = Server::tools(calls.clone()).await;
            let session = super::approvals::prepare(&client, &server, &root).await;
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
            let page = finished(&client, session.id, turn.id).await;
            super::evaluation::trajectory(&page, &calls);
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                page.runs
            );
            let subscription = tokio::time::timeout(Duration::from_secs(2), browser)
                .await
                .unwrap()
                .unwrap();
            let requests = server.requests.lock().unwrap().clone();
            let tools = requests[0]["tools"].as_array().unwrap();
            for (name, _) in calls {
                assert!(tools.iter().any(|tool| tool["function"]["name"] == name));
            }
            if mode == WorkMode::Plan {
                for name in mutating {
                    assert!(
                        !tools
                            .iter()
                            .any(|tool| tool["function"]["name"] == plugin_tool("browser", name))
                    );
                }
                assert!(page.approvals.is_empty());
            } else {
                assert_eq!(page.approvals.len(), mutating.len());
            }
            let results: Vec<_> = page
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter_map(|part| {
                    if let Part::ToolResult { result, .. } = part {
                        Some(result)
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(results.len(), count);
            assert!(
                results
                    .iter()
                    .all(|result| result["text"] == "Browser fixture")
            );
            drop(subscription);
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn publishes_controller_artifact() {
    use base64::Engine;

    let pixels = image::RgbImage::from_pixel(2, 2, image::Rgb([32, 96, 160]));
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode_image(&pixels)
        .unwrap();
    let encoded = base64::engine::general_purpose::STANDARD.encode(&jpeg);
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
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
        let server = Server::tools(vec![(name.clone(), json!({"tab":7}))]).await;
        let session = approvals::prepare(&client, &server, &root).await;
        let turn = approvals::submit(&client, session.id).await;
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
        assert_eq!(call.session, session.id);
        assert_eq!(
            call.action,
            sailry_protocol::browser::Action::Screenshot { tab: 7 }
        );
        // Synthetic pixels exercise publishing without calling a screen capture API.
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
        let page = finished(&client, session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(page.approvals.len(), 1);
        let path = format!("assets/generated/browser-{}.jpg", call.id);
        let expected = json!({
            "tab":7,"mime_type":"image/jpeg","width":2,"height":2,"path":path
        });
        let results: Vec<_> = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| match part {
                Part::ToolResult {
                    name: tool,
                    result,
                    images,
                    ..
                } if tool == &name => {
                    assert!(images.is_empty());
                    Some(result)
                }
                _ => None,
            })
            .collect();
        assert_eq!(results, vec![&expected]);
        assert!(
            !serde_json::to_string(&page)
                .unwrap()
                .contains("base64_image")
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
        assert_eq!(download.path, path);
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
        let requests = server.requests.lock().unwrap().clone();
        let message = requests.last().unwrap()["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|message| message["role"] == "tool")
            .expect("canonical tool result must reach the next model request");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(message["content"].as_str().unwrap())
                .unwrap(),
            expected
        );
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
        let node = Node::start(directory.path().join("node")).await.unwrap();
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
        let session = super::approvals::prepare(&client, &server, &root).await;
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
        let node = Node::start(directory.path().join("node")).await.unwrap();
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
        let session = super::approvals::prepare(&client, &server, &root).await;
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
        let node = Node::start(directory.path().join("node")).await.unwrap();
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
        let session = super::approvals::prepare(&client, &server, &root).await;

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
                    !tool["function"]["name"]
                        .as_str()
                        .unwrap_or_default()
                        .starts_with(&plugin_tool("browser", ""))
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
        let node = Node::start(directory.path().join("node")).await.unwrap();
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
        let session = super::approvals::prepare(&client, &server, &root).await;
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
