//! Opt-in real Chromium acceptance; the model is a deterministic fixture.
#[path = "external_browser/pages.rs"]
mod pages;
use super::*;
use serde_json::json;

#[tokio::test]
#[ignore = "downloads pinned Chrome for Testing and runs real browser processes"]
async fn managed_workflows() {
    workflow(true).await;
}

#[tokio::test]
#[ignore = "downloads pinned Chrome for Testing and runs real browser processes"]
async fn managed_interactions() {
    workflow(false).await;
}

async fn workflow(capture: bool) {
    for remote in [false, true] {
        let pages = pages::Pages::start().await;
        let mut calls = vec![
            ("external_browser_navigate".into(), json!({"url":pages.url})),
            (
                "external_browser_type".into(),
                json!({"selector":"#name","text":"Alice"}),
            ),
            (
                "external_browser_select".into(),
                json!({"selector":"#choice","value":"b"}),
            ),
            (
                "external_browser_click".into(),
                json!({"selector":"#submit"}),
            ),
            (
                "external_browser_extract_text".into(),
                json!({"selector":"#result"}),
            ),
            (
                "external_browser_switch_to_frame".into(),
                json!({"selector":"#frame"}),
            ),
            (
                "external_browser_type".into(),
                json!({"selector":"#inside","text":"Frame input"}),
            ),
            (
                "external_browser_evaluate_js".into(),
                json!({"script":"return document.querySelector('#inside').value"}),
            ),
            (
                "external_browser_switch_to_default_content".into(),
                json!({}),
            ),
            (
                "external_browser_file_upload".into(),
                json!({"selector":"#upload","file_path":"upload.txt"}),
            ),
            (
                "external_browser_wait_for_text".into(),
                json!({"text":"Uploaded","timeout":5}),
            ),
            (
                "external_browser_click".into(),
                json!({"selector":"#download"}),
            ),
            ("external_browser_wait".into(), json!({"seconds":1})),
            ("external_browser_downloads".into(), json!({})),
            (
                "external_browser_save_download".into(),
                json!({"name":"fixture.txt"}),
            ),
            (
                "external_browser_evaluate_js".into(),
                json!({"script":"localStorage.setItem('fixture','persisted');return true"}),
            ),
            ("external_browser_close_session".into(), json!({})),
            ("external_browser_navigate".into(), json!({"url":pages.url})),
            (
                "external_browser_evaluate_js".into(),
                json!({"script":"return localStorage.getItem('fixture')"}),
            ),
            (
                "external_browser_file_upload".into(),
                json!({"selector":"#upload","file_path":"../outside.txt"}),
            ),
            (
                "external_browser_save_download".into(),
                json!({"name":"../profile/Preferences"}),
            ),
        ];
        if capture {
            calls.extend([
                ("external_browser_screenshot".into(), json!({})),
                (
                    "external_browser_screenshot".into(),
                    json!({"selector":"#name"}),
                ),
            ]);
        }
        let count = calls.len();
        let fixture = Fixture::start(remote, calls).await;
        let Fixture {
            client,
            session,
            turn,
            root,
            node,
            ..
        } = &fixture;
        let page = tokio::time::timeout(Duration::from_secs(600), async {
            loop {
                let page = history(client, *session).await;
                if page.runs.iter().any(|run| {
                    run.turn == *turn
                        && !matches!(
                            run.status,
                            Status::Queued | Status::Running | Status::Stopping
                        )
                }) {
                    break page;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("managed browser workflow deadline");
        let results: Vec<_> = results(client, &page).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{results:?}"
        );
        assert_eq!(results.len(), count, "{results:?}");
        for part in page.entries.iter().flat_map(|entry| &entry.parts) {
            if let Part::ToolCall { name, summary, .. } = part {
                assert!(name.starts_with(&plugin_tool("external-browser", "browser_")));
                let display = summary.resolved.as_ref().expect("packaged browser display");
                assert!(
                    display
                        .label
                        .as_ref()
                        .unwrap()
                        .label
                        .starts_with("External browser")
                );
                assert_eq!(
                    summary.icon,
                    Some(sailry_protocol::plugin::desktop::Icon::Name(
                        "reicon:map/globe".into()
                    ))
                );
            }
        }
        for result in &results[..19] {
            assert_eq!(result["execution_node"], json!(node.id()), "{result}");
        }
        assert_eq!(results[4]["result"]["text"], "Alice:b");
        assert_eq!(results[7]["result"]["result"], "Frame input");
        assert_eq!(results[9]["result"]["uploaded_file"], "upload.txt");
        assert_eq!(*pages.upload.lock().unwrap(), b"upload fixture");
        assert!(
            results[13]["files"]
                .as_array()
                .unwrap()
                .iter()
                .any(|file| file["name"] == "fixture.txt")
        );
        let saved = results[14]["path"].as_str().unwrap();
        assert!(saved.starts_with("assets/generated/browser-"), "{saved}");
        assert_eq!(
            std::fs::read_to_string(root.join(saved)).unwrap(),
            "download fixture"
        );
        assert_eq!(results[18]["result"]["result"], "persisted");
        for result in &results[19..21] {
            assert!(result.get("error").is_some(), "{result}");
        }
        for result in &results[21..] {
            assert_eq!(result["execution_node"], json!(node.id()));
            assert_eq!(result["result"]["success"], true, "{result}");
            assert!(result["result"].get("base64_image").is_none());
            let saved = result["result"]["path"].as_str().unwrap();
            assert!(saved.starts_with("assets/generated/browser-"), "{saved}");
            let bytes = std::fs::read(root.join(saved)).unwrap();
            assert_eq!(
                image::guess_format(&bytes).unwrap(),
                image::ImageFormat::Png
            );
            let image = image::load_from_memory(&bytes).unwrap();
            assert!(image.width() > 0 && image.height() > 0);
        }
        assert_eq!(*pages.upload.lock().unwrap(), b"upload fixture");
        let persistent = fixture
            .profile()
            .join("browser/sessions")
            .join(session.to_string());
        assert!(persistent.join("profile").is_dir());
        assert!(!persistent.join("downloads").exists());
        let temporary: Vec<_> = std::fs::read_dir(fixture.profile().join("tmp"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("browser-")
            })
            .collect();
        assert_eq!(temporary.len(), 1);
        assert!(temporary[0].join("downloads").is_dir());
        assert_eq!(
            std::fs::read_dir(temporary[0].join("downloads"))
                .unwrap()
                .count(),
            0
        );
        assert!(!processes(&fixture.profile()).is_empty());
        fixture.shutdown().await;
    }
}

#[tokio::test]
#[ignore = "downloads pinned Chrome for Testing and runs real browser processes"]
async fn cancellation_closes_processes() {
    for remote in [false, true] {
        let pages = pages::Pages::start().await;
        let fixture = Fixture::start(
            remote,
            vec![
                ("external_browser_navigate".into(), json!({"url":pages.url})),
                (
                    "external_browser_evaluate_js".into(),
                    json!({"script":"fetch('/waiting');", "async":true}),
                ),
            ],
        )
        .await;
        tokio::time::timeout(Duration::from_secs(600), pages.waiting.notified())
            .await
            .expect("browser reached the cancellable script");
        assert!(!processes(&fixture.profile()).is_empty());
        fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::StopTurn { turn: fixture.turn }),
            )
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session, fixture.turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Cancelled);
        stopped(&fixture.profile()).await;
        fixture.shutdown().await;
    }
}

#[tokio::test]
async fn filters_without_provisioning() {
    for remote in [false, true] {
        for enabled in [false, true] {
            for mode in [WorkMode::Code, WorkMode::Plan] {
                let denied = enabled && mode == WorkMode::Plan;
                let mut calls = if enabled { discovery() } else { vec![] };
                if denied {
                    calls.push((
                        "external_browser_click".into(),
                        json!({"selector":"#submit"}),
                    ));
                }
                let fixture =
                    Fixture::configured(remote, calls, mode, Permission::Full, enabled, true).await;
                let page = if mode == WorkMode::Plan {
                    questions::finish_plan(&fixture.client, fixture.session, fixture.turn).await
                } else {
                    finished(&fixture.client, fixture.session, fixture.turn).await
                };
                if denied {
                    planning_denial(&page, fixture.turn);
                } else {
                    assert_eq!(page.runs.last().unwrap().status, Status::Completed);
                }
                assert!(page.approvals.is_empty());
                {
                    let requests = fixture._server.requests.lock().unwrap();
                    let prefix = plugin_tool("external-browser", "browser_");
                    assert!(requests[0]["tools"].as_array().unwrap().iter().all(|tool| {
                        !tool["function"]["name"]
                            .as_str()
                            .is_some_and(|name| name.starts_with(&prefix))
                    }));
                    let tools = requests.last().unwrap()["tools"].as_array().unwrap();
                    let count = tools
                        .iter()
                        .filter(|tool| {
                            tool["function"]["name"]
                                .as_str()
                                .is_some_and(|name| name.starts_with(&prefix))
                        })
                        .count();
                    assert_eq!(count, if enabled { 49 } else { 0 });
                    let contains = |name| {
                        tools
                            .iter()
                            .any(|entry| entry["function"]["name"] == tool(name))
                    };
                    assert_eq!(contains("external_browser_navigate"), enabled);
                    assert_eq!(contains("external_browser_click"), enabled);
                    assert_eq!(contains("external_browser_file_upload"), enabled);
                    assert!(!fixture.profile().join("tools/chromium").exists());
                }
                fixture.shutdown().await;
            }
        }
    }
}

#[tokio::test]
async fn requires_provisioning_authority() {
    for remote in [false, true] {
        for disable in [false, true] {
            let fixture = Fixture::configured(
                remote,
                vec![
                    (
                        "tool_search".into(),
                        json!({"query":"browser click","limit":8}),
                    ),
                    (
                        "external_browser_click".into(),
                        json!({"selector":"#submit"}),
                    ),
                ],
                WorkMode::Code,
                Permission::Ask,
                true,
                true,
            )
            .await;
            let (_, approval) = super::approvals::pending(&fixture.client, fixture.session).await;
            if disable {
                let package = package(&fixture.client).await;
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::SetPluginEnabled {
                        name: package.name,
                        expected_revision: package.revision,
                        enabled: false,
                    }))
                    .await
                    .unwrap();
            }
            fixture
                .client
                .execute(fixture.client.prepare(Command::ResolveApproval {
                    session: fixture.session,
                    approval: approval.id,
                    decision: if disable {
                        Decision::Approve
                    } else {
                        Decision::Deny
                    },
                }))
                .await
                .unwrap();
            let page = finished(&fixture.client, fixture.session, fixture.turn).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
            let details = tool_details(&fixture.client, &page).await;
            let result = details
                .iter()
                .find(|(name, _)| *name == tool("external_browser_click"))
                .unwrap()
                .1
                .result
                .as_ref()
                .unwrap();
            if disable {
                assert_eq!(result["isError"], true, "{result}");
                let fault: Fault = serde_json::from_value(result["error"].clone()).unwrap();
                assert_eq!(fault.code, ErrorCode::NotConfigured);
                assert_eq!(fault.message, "plugin is disabled");
            } else {
                assert_eq!(
                    result,
                    &serde_json::to_value(rmcp::model::CallToolResult::error(vec![
                        rmcp::model::ContentBlock::text("Tool execution was denied."),
                    ]))
                    .unwrap()
                );
            }
            assert!(!fixture.profile().join("tools/chromium").exists());
            fixture.shutdown().await;
        }
    }
}

struct Fixture {
    directory: tempfile::TempDir,
    root: std::path::PathBuf,
    node: Node,
    controller: Link,
    client: Client,
    session: SessionId,
    turn: TurnId,
    _server: Server,
}

impl Fixture {
    async fn start(remote: bool, calls: Vec<(String, serde_json::Value)>) -> Self {
        Self::configured(remote, calls, WorkMode::Code, Permission::Full, true, false).await
    }

    async fn configured(
        remote: bool,
        calls: Vec<(String, serde_json::Value)>,
        mode: WorkMode,
        permission: Permission,
        enabled: bool,
        minimal: bool,
    ) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("upload.txt"), "upload fixture").unwrap();
        std::fs::write(directory.path().join("outside.txt"), "outside fixture").unwrap();
        let node = if minimal {
            Node::start_empty(directory.path().join("node")).await
        } else {
            Node::start(directory.path().join("node")).await
        }
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
        if minimal {
            let Output::Plugin(package) = client
                .execute(client.prepare(Command::InstallBundledPlugin {
                    name: "external-browser".into(),
                    expected_revision: 0,
                }))
                .await
                .unwrap()
            else {
                panic!("external browser package expected")
            };
            assert!(package.issues.is_empty(), "{:?}", package.issues);
            // Explicit installation starts enabled; keep the fixture's initial
            // disabled configuration without installing unrelated packages.
            client
                .execute(client.prepare(Command::SetPluginEnabled {
                    name: package.summary.name,
                    expected_revision: package.summary.revision,
                    enabled: false,
                }))
                .await
                .unwrap();
        }
        let package = package(&client).await;
        assert!(!package.enabled, "managed browser starts disabled");
        if enabled {
            client
                .execute(client.prepare(Command::SetPluginEnabled {
                    name: package.name,
                    expected_revision: package.revision,
                    enabled: true,
                }))
                .await
                .unwrap();
        }
        let server = Server::tools(
            calls
                .into_iter()
                .map(|(name, arguments)| {
                    let name = if name == "tool_search" {
                        name
                    } else {
                        tool(&name)
                    };
                    (name, arguments)
                })
                .collect(),
        )
        .await;
        let session = if minimal {
            let (session, provider) = configured_session(&client, &server.endpoint, &root).await;
            super::approvals::prepare_model(&client, session, provider).await
        } else {
            super::approvals::prepare(&client, &server, &root).await
        };
        let mut config = session.config.clone();
        config.permission = permission;
        config.mode = mode;
        client
            .execute(client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config,
            }))
            .await
            .unwrap();
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: 2,
                message: "Exercise the external browser".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };

        Self {
            directory,
            root,
            node,
            controller,
            client,
            session: session.id,
            turn: turn.id,
            _server: server,
        }
    }

    fn profile(&self) -> std::path::PathBuf {
        self.directory.path().join("node").canonicalize().unwrap()
    }

    async fn shutdown(self) {
        let profile = self.profile();
        let temporary: Vec<_> = std::fs::read_dir(profile.join("tmp"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("browser-")
            })
            .collect();
        let persistent = profile
            .join("browser/sessions")
            .join(self.session.to_string())
            .join("profile");
        let retained = persistent.exists();
        drop(self.client);
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
        for path in temporary {
            assert!(!path.exists());
        }
        if retained {
            assert!(persistent.is_dir());
        }
        stopped(&profile).await;
    }
}

fn discovery() -> Vec<(String, serde_json::Value)> {
    // Each search admits at most eight schemas; together these capabilities
    // exercise all 49 declarations without executing or provisioning a browser.
    [
        "upload",
        "for",
        "window",
        "element",
        "to",
        "text",
        "page",
        "tab",
        "load",
        "wait",
        "scroll",
        "screenshot",
        "refresh",
        "navigate",
        "links",
        "hover",
        "handle",
        "get",
        // Core management tools share the limit for broad capability searches.
        "delete_cookie",
        "evaluate",
        "focus",
    ]
    .into_iter()
    .map(|query| ("tool_search".into(), json!({"query":query,"limit":8})))
    .collect()
}

fn tool(name: &str) -> String {
    plugin_tool("external-browser", name.strip_prefix("external_").unwrap())
}

async fn package(client: &Client) -> sailry_protocol::plugin::Summary {
    let Output::Plugins(packages) = client
        .execute(client.prepare(Command::ListPlugins))
        .await
        .unwrap()
    else {
        panic!("plugins expected")
    };
    packages
        .into_iter()
        .find(|package| package.name == "external-browser")
        .unwrap()
}

fn processes(profile: &std::path::Path) -> Vec<sysinfo::Pid> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
    );
    system
        .processes()
        .values()
        .filter(|process| process.exe().is_some_and(|path| path.starts_with(profile)))
        .map(|process| process.pid())
        .collect()
}

async fn stopped(profile: &std::path::Path) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if processes(profile).is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("managed browser processes exited");
}
