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
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{results:?}"
        );
        assert_eq!(results.len(), count, "{results:?}");
        for part in page.entries.iter().flat_map(|entry| &entry.parts) {
            if let Part::ToolCall { name, display, .. } = part {
                assert!(name.starts_with(&plugin_tool("external-browser", "browser_")));
                let display = display.as_ref().expect("packaged browser display");
                assert!(display.label.starts_with("External browser"));
                assert_eq!(
                    display.icon,
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
                let fixture =
                    Fixture::configured(remote, vec![], mode, Permission::Full, enabled).await;
                let page = finished(&fixture.client, fixture.session, fixture.turn).await;
                assert_eq!(page.runs.last().unwrap().status, Status::Completed);
                {
                    let requests = fixture._server.requests.lock().unwrap();
                    let tools = requests[0]["tools"].as_array().unwrap();
                    let prefix = plugin_tool("external-browser", "browser_");
                    let count = tools
                        .iter()
                        .filter(|tool| {
                            tool["function"]["name"]
                                .as_str()
                                .is_some_and(|name| name.starts_with(&prefix))
                        })
                        .count();
                    assert_eq!(
                        count,
                        if enabled {
                            if mode == WorkMode::Plan { 19 } else { 49 }
                        } else {
                            0
                        }
                    );
                    let contains = |name| {
                        tools
                            .iter()
                            .any(|entry| entry["function"]["name"] == tool(name))
                    };
                    assert_eq!(contains("external_browser_navigate"), enabled);
                    assert_eq!(
                        contains("external_browser_click"),
                        enabled && mode == WorkMode::Code
                    );
                    assert_eq!(
                        contains("external_browser_file_upload"),
                        enabled && mode == WorkMode::Code
                    );
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
                vec![(
                    "external_browser_click".into(),
                    json!({"selector":"#submit"}),
                )],
                WorkMode::Code,
                Permission::Ask,
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
            assert!(
                page.entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| matches!(part,
                Part::ToolResult { result, .. } if result.get("error").is_some()))
            );
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
        Self::configured(remote, calls, WorkMode::Code, Permission::Full, true).await
    }

    async fn configured(
        remote: bool,
        calls: Vec<(String, serde_json::Value)>,
        mode: WorkMode,
        permission: Permission,
        enabled: bool,
    ) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("upload.txt"), "upload fixture").unwrap();
        std::fs::write(directory.path().join("outside.txt"), "outside fixture").unwrap();
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
                .map(|(name, arguments)| (tool(&name), arguments))
                .collect(),
        )
        .await;
        let session = super::approvals::prepare(&client, &server, &root).await;
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
        drop(self.client);
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
        stopped(&profile).await;
    }
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
