//! Flutter UI acceptance against an isolated, real remote Node and shared Rust Client.
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{
    conversation::{Model, ModelApi, Part, Provider, Status},
    *,
};
use serde_json::json;
use std::{
    path::Path,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[allow(dead_code)]
#[path = "../../node-runtime/tests/agent_support/mod.rs"]
mod support;

#[path = "flutter/android.rs"]
mod android;

const CONTENT: &str = "Flutter reads this file 中文 🙂";
const MODEL: &str = "flutter-fixture";

#[test]
#[ignore = "requires Flutter dependencies/native bridge; SAILRY_FLUTTER_DEVICE selects a simulator"]
fn controls_remote_node() {
    let device = std::env::var("SAILRY_FLUTTER_DEVICE").ok();
    let profile = std::env::var("SAILRY_FLUTTER_PROFILE").as_deref() == Ok("1");
    let emulator = device
        .as_deref()
        .is_some_and(|id| id.starts_with("emulator-"));
    let android = emulator || std::env::var("SAILRY_FLUTTER_ANDROID").as_deref() == Ok("1");
    assert!(
        !profile || device.is_some() && !emulator,
        "profile measurements require a physical device"
    );
    let adb = std::env::var("SAILRY_ADB").unwrap_or_else(|_| "adb".into());
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let temporary = root.join(".runtime/mobile-flutter");
    std::fs::create_dir_all(&temporary).unwrap();
    let directory = tempfile::Builder::new()
        .prefix("node-")
        .tempdir_in(&temporary)
        .unwrap();
    let project = directory.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("source.txt"), CONTENT).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (node, server, unassigned, text_server, bootstrap, fixture) = runtime.block_on(async {
        let server = support::Server::tools(vec![(support::plugin_tool("files", "read_file"), json!({"path":"source.txt"}))]).await;
        let network = if android && !emulator {
            sailry_node_runtime::NetworkScope::Internet
        } else if android {
            sailry_node_runtime::NetworkScope::Direct(([0, 0, 0, 0], 0).into())
        } else {
            sailry_node_runtime::NetworkScope::default()
        };
        let node = Node::start_with_network(directory.path().join("node"), network.clone())
            .await
            .unwrap();
        configure(&Client::new(node.local()), &server.endpoint).await;
        let text_server = support::Server::start(false).await;
        let unassigned = Node::start_with_network(directory.path().join("unassigned"), network).await.unwrap();
        configure(&Client::new(unassigned.local()), &text_server.endpoint).await;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let token = RequestId::new().to_string();
        // The emulator maps host loopback; a physical phone uses one isolated
        // adb reverse port for bootstrap. Link still uses the real Node endpoint.
        let address = listener.local_addr().unwrap();
        let host = if emulator {
            "10.0.2.2"
        } else {
            "127.0.0.1"
        };
        let url = format!("http://{host}:{}/{token}", address.port());
        let link = node.link();
        let unassigned_link = unassigned.link();
        let input_device = device.clone();
        let input_adb = adb.clone();
        // Invitations expire after one minute. Issue it only after Flutter has
        // compiled/launched, without extending the real pairing lifetime.
        let fixture = tokio::spawn(async move {
            for (path, link) in [(format!("/{token}/unassigned"), unassigned_link), (format!("/{token}"), link)] {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = vec![0; 4096];
            let count = stream.read(&mut bytes).await.unwrap();
            let request = std::str::from_utf8(&bytes[..count]).unwrap();
            assert!(request.starts_with(&format!("GET {path} HTTP/1.1\r\n")));
            let mut invitation = link.invite().unwrap();
            let body = json!({"invitation":invitation.ticket()}).to_string();
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
            invitation.paired().await.unwrap();
            if android && path.ends_with("/unassigned") {
                let (mut stream, _) = listener.accept().await.unwrap();
                let count = stream.read(&mut bytes).await.unwrap();
                let request = std::str::from_utf8(&bytes[..count]).unwrap();
                assert!(request.starts_with(&format!("GET /{token}/ime HTTP/1.1\r\n")));
                let adb = input_adb.clone();
                let device = input_device.clone().unwrap();
                tokio::task::spawn_blocking(move || android::enter_input(&adb, &device)).await.unwrap();
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
                stream.shutdown().await.unwrap();
            }
            }
        });
        (node, server, unassigned, text_server, (url, address.port()), fixture)
    });
    let (bootstrap, port) = bootstrap;
    let reverse = android && !emulator;
    if reverse {
        assert!(
            std::process::Command::new(&adb)
                .args([
                    "-s",
                    device.as_deref().unwrap(),
                    "reverse",
                    &format!("tcp:{port}"),
                    &format!("tcp:{port}")
                ])
                .status()
                .unwrap()
                .success(),
            "physical Android fixture bootstrap"
        );
    }
    let mut command = std::process::Command::new("flutter");
    command.current_dir(root.join("apps/mobile"));
    if profile {
        command.args([
            "drive",
            "--no-pub",
            "--profile",
            "--no-dds",
            "--keep-app-running",
            "--driver=integration_test/driver.dart",
            "--target=integration_test/node_test.dart",
        ]);
    } else {
        command.args(["test", "--no-pub", "--no-uninstall", "--reporter=expanded"]);
    }
    if android {
        command.env("SAILRY_ANDROID_TEST_APP", "1");
        command.arg("--dart-define=SAILRY_ANDROID_IME_ACCEPTANCE=true");
    }
    if let Some(device) = device {
        if !profile {
            command.arg("integration_test/node_test.dart");
        }
        command.args(["-d", &device]);
    } else {
        // This wrapper runs the same real-network flow in flutter-tester,
        // without claiming iOS/Android device acceptance.
        command.arg("test/native_node_test.dart");
        let name = if cfg!(target_os = "macos") {
            "libsailry_mobile_bridge.dylib"
        } else if cfg!(target_os = "windows") {
            "sailry_mobile_bridge.dll"
        } else {
            "libsailry_mobile_bridge.so"
        };
        command.arg(format!(
            "--dart-define=SAILRY_BRIDGE_LIBRARY={}",
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join(name)
                .display()
        ));
    }
    command
        .arg(format!("--dart-define=SAILRY_FLUTTER_FIXTURE={bootstrap}"))
        .arg(format!(
            "--dart-define=SAILRY_PROJECT_PATH={}",
            project.display()
        ))
        .arg(format!(
            "--dart-define=SAILRY_CONTROLLER_PROFILE={}",
            directory.path().join("controller").display()
        ))
        .arg(format!(
            "--dart-define=SAILRY_FLUTTER_RUN={}",
            RequestId::new()
        ));
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(25 * 60);
    let mut terminal_claimed = false;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            runtime.block_on(node.shutdown()).unwrap();
            runtime.block_on(unassigned.shutdown()).unwrap();
            panic!("Flutter acceptance deadline");
        }
        if !terminal_claimed {
            let client = Client::new(unassigned.local());
            if let Output::Snapshot(snapshot) = runtime
                .block_on(client.execute(client.prepare(Command::Snapshot)))
                .unwrap()
                && let Some(terminal) = snapshot.terminals.first()
            {
                runtime
                    .block_on(client.execute(client.prepare(Command::ClaimTerminal {
                        terminal: terminal.id,
                        expected_revision: terminal.revision,
                    })))
                    .unwrap();
                terminal_claimed = true;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    if reverse {
        assert!(
            std::process::Command::new(&adb)
                .args([
                    "-s",
                    std::env::var("SAILRY_FLUTTER_DEVICE").unwrap().as_str(),
                    "reverse",
                    "--remove",
                    &format!("tcp:{port}")
                ])
                .status()
                .unwrap()
                .success(),
            "remove isolated Android bootstrap forwarding"
        );
    }
    let page = runtime.block_on(async {
        if !status.success() {
            fixture.abort();
            node.shutdown().await.unwrap();
            unassigned.shutdown().await.unwrap();
            panic!("Flutter UI acceptance failed");
        }
        fixture.await.unwrap();
        let empty_client = Client::new(unassigned.local());
        let Output::Snapshot(empty) = empty_client
            .execute(empty_client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("unassigned snapshot expected")
        };
        assert!(empty.projects.is_empty());
        assert_eq!(empty.sessions.len(), 1);
        assert!(empty.sessions[0].project.is_none());
        assert!(
            empty
                .worktrees
                .iter()
                .any(|tree| tree.id == empty.sessions[0].worktree)
        );
        let unassigned_page = empty_client
            .read_conversation(empty.sessions[0].id, None, 100)
            .await
            .unwrap()
            .page;
        assert_eq!(unassigned_page.runs.len(), 1);
        assert_eq!(unassigned_page.runs[0].status, Status::Completed);
        assert_eq!(text_server.requests.lock().unwrap().len(), 1);
        assert!(
            terminal_claimed,
            "terminal control transferred from the execution Node"
        );
        unassigned.shutdown().await.unwrap();
        let client = Client::new(node.local());
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert_eq!(
            snapshot.projects.len(),
            1,
            "project admitted from Flutter UI"
        );
        assert_eq!(snapshot.projects[0].name, "Flutter fixture");
        assert_eq!(snapshot.sessions.len(), 1, "one UI-created conversation");
        let page = client
            .read_conversation(snapshot.sessions[0].id, None, 100)
            .await
            .unwrap()
            .page;
        node.shutdown().await.unwrap();
        page
    });
    assert_eq!(page.runs.len(), 1);
    assert_eq!(page.runs[0].status, Status::Completed);
    assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part| matches!(part, Part::ToolResult { name, result, .. } if name == &support::plugin_tool("files", "read_file") && result["data"]["text"] == CONTENT)), "actual filesystem contents reach canonical tool history");
    let requests = server.requests.lock().unwrap();
    assert_eq!(
        requests.len(),
        2,
        "reopening the controller must not execute again"
    );
    assert!(requests.iter().all(|request| request["model"] == MODEL));
    assert!(
        requests[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "tool"
                && message["content"]
                    .as_str()
                    .is_some_and(|content| content.contains(CONTENT))),
        "real tool output returns to the model request"
    );
}

async fn configure(client: &Client, endpoint: &str) {
    let Output::Provider(provider) = client
        .execute(client.prepare(Command::SaveProvider {
            expected_revision: 0,
            secret: Some(Secret::new("isolated-flutter-fixture".into())),
            provider: Provider {
                id: ProviderId::new(),
                revision: 0,
                name: "Flutter fixture".into(),
                api: ModelApi::ChatCompletions,
                authentication: Authentication::ApiKey,
                endpoint: endpoint.into(),
                options: None,
                oauth: None,
                enabled: true,
                default_model: MODEL.into(),
                credential: None,
                models: vec![Model {
                    id: MODEL.into(),
                    context: 4096,
                    output: 128,
                    vision: false,
                    tools: true,
                    reasoning: false,
                    web_search: false,
                    generates: vec![],
                    efforts: vec![],
                    custom_efforts: false,
                    default_effort: Effort::Default,
                }],
            },
        }))
        .await
        .unwrap()
    else {
        panic!("provider expected")
    };
    client
        .execute(client.prepare(Command::SetDefaults {
            expected_revision: 0,
            config: SessionConfig {
                assistant: None,
                resource: None,
                provider: provider.id,
                model: MODEL.into(),
                effort: Effort::Default,
                mode: WorkMode::Code,
                permission: Permission::Ask,
                credential: provider.credential,
            },
        }))
        .await
        .unwrap();
}
