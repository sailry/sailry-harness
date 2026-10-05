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

const CONTENT: &str = "Flutter reads this file 中文 🙂";
const MODEL: &str = "flutter-fixture";

#[test]
#[ignore = "requires Flutter dependencies/native bridge; SAILRY_FLUTTER_DEVICE selects a simulator"]
fn controls_remote_node() {
    let device = std::env::var("SAILRY_FLUTTER_DEVICE").ok();
    let android = device
        .as_deref()
        .is_some_and(|id| id.starts_with("emulator-"));
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
    let (node, server, bootstrap, fixture) = runtime.block_on(async {
        let server = support::Server::tools(vec![(support::plugin_tool("files", "read_file"), json!({"path":"source.txt"}))]).await;
        let network = if android {
            sailry_node_runtime::NetworkScope::Direct(([0, 0, 0, 0], 0).into())
        } else {
            sailry_node_runtime::NetworkScope::default()
        };
        let node = Node::start_with_network(directory.path().join("node"), network)
            .await
            .unwrap();
        configure(&Client::new(node.local()), &server.endpoint).await;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let token = RequestId::new().to_string();
        // Android's emulator maps this address to the host loopback interface.
        // Only the isolated test bootstrap uses it; Link advertises its real addresses.
        let address = listener.local_addr().unwrap();
        let host = if android {
            "10.0.2.2"
        } else {
            "127.0.0.1"
        };
        let url = format!("http://{host}:{}/{token}", address.port());
        let link = node.link();
        // Invitations expire after one minute. Issue it only after Flutter has
        // compiled/launched, without extending the real pairing lifetime.
        let fixture = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = vec![0; 4096];
            let count = stream.read(&mut bytes).await.unwrap();
            let request = std::str::from_utf8(&bytes[..count]).unwrap();
            assert!(request.starts_with(&format!("GET /{token} HTTP/1.1\r\n")));
            let mut invitation = link.invite().unwrap();
            let body = json!({"invitation":invitation.ticket()}).to_string();
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
            invitation.paired().await.unwrap();
        });
        (node, server, url, fixture)
    });
    let mut command = std::process::Command::new("flutter");
    command
        .current_dir(root.join("apps/mobile"))
        .args(["test", "--no-pub", "--reporter=expanded"]);
    if let Some(device) = device {
        command.args(["integration_test/node_test.dart", "-d", &device]);
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
            root.join("target/debug").join(name).display()
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
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            runtime.block_on(node.shutdown()).unwrap();
            panic!("Flutter acceptance deadline");
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let page = runtime.block_on(async {
        if !status.success() {
            fixture.abort();
            node.shutdown().await.unwrap();
            panic!("Flutter UI acceptance failed");
        }
        fixture.await.unwrap();
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
