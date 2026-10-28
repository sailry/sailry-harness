use super::*;
use crate::Node;
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_protocol::{NodeId, Output, WorktreeId};

fn context() -> CallContext {
    CallContext::new(
        "session".into(),
        "call",
        &rmcp::model::CallToolRequestParams::new("read-fixture"),
        CancellationToken::new(),
        Default::default(),
    )
    .unwrap()
}

struct Fixture {
    directory: tempfile::TempDir,
    node: Node,
    controller: Link,
    client: Client,
    tool: HostOperation,
}

impl Fixture {
    async fn new(remote: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty_with_plugin_catalog(
            directory.path().join("node"),
            "http://127.0.0.1:9/",
        )
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
        let transport = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let caller = if remote {
            NodeId(*controller.handle().address().id.as_bytes())
        } else {
            node.id()
        };
        let client = Client::new(transport.clone());
        let root = directory.path().join("source");
        std::fs::create_dir_all(root.join("package")).unwrap();
        std::fs::write(root.join("notes.txt"), "Read once\n").unwrap();
        std::fs::write(
            root.join("package/plugin.json"),
            json!({
                "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
                "name": "read-fixture", "version": "0.1.0", "description": "Read fixture",
                "extensions": {"dev.sailry.platform": {
                    "api_version": "v1", "actions": [plugin::Action::ReadFiles, plugin::Action::ReadStorage]
                }}
            })
            .to_string(),
        )
        .unwrap();
        client
            .execute(client.prepare(Command::RegisterProject {
                name: "Read fixture".into(),
                path: root.to_str().unwrap().into(),
            }))
            .await
            .unwrap();
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let worktree = snapshot.worktrees[0].id;
        let Output::Plugin(package) = client
            .execute(client.prepare(Command::InstallPlugin {
                worktree,
                path: "package".into(),
                name: "read-fixture".into(),
                expected_revision: 0,
                expected_digest: None,
            }))
            .await
            .unwrap()
        else {
            panic!("plugin expected")
        };
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let tool = HostOperation {
            ingress: node.ingress.clone(),
            transport,
            turn: TurnId::new(),
            caller,
            context: plugin::Context {
                invocation: None,
                turn: None,
                surface: plugin::desktop::Surface::Workspace,
                package: package.summary.reference(),
                worktree: Some(worktree),
                session: None,
            },
            name: "read-fixture".into(),
            description: "Read fixture".into(),
            operation: Operation::ReadFile,
            settings: None,
            connections: Vec::new(),
            stop: CancellationToken::new(),
            managed: false,
        };
        Self {
            directory,
            node,
            controller,
            client,
            tool,
        }
    }

    async fn read(&self, arguments: Value) -> Value {
        self.tool
            .execute_bound(Arc::new(context()), arguments.clone(), arguments)
            .await
            .unwrap()
    }

    async fn close(self) {
        drop(self.tool);
        drop(self.client);
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn checks_each_routed_read_once() {
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote).await;
        let before = fixture.node.ingress.plugin_check_count();
        let output = fixture.read(json!({"path": "notes.txt"})).await;
        assert!(output.get("error").is_none(), "{output}");
        assert_eq!(output["data"]["lines"][0]["text"], "Read once\n");
        assert_eq!(fixture.node.ingress.plugin_check_count() - before, 1);

        fixture.tool.operation = Operation::GetValue;
        let before = fixture.node.ingress.plugin_check_count();
        let output = fixture.read(json!({"key": "missing"})).await;
        assert!(output.get("error").is_none(), "{output}");
        assert_eq!(output["data"]["present"], false);
        assert_eq!(fixture.node.ingress.plugin_check_count() - before, 1);
        fixture.close().await;
    }
}

#[tokio::test]
async fn preserves_scope_and_live_revocation() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let arguments = json!({"path": "notes.txt"});
        let request = fixture.tool.prepare(arguments.clone()).await.unwrap();
        let mut wrong = request.clone();
        wrong.target = NodeId([9; 32]);
        assert_eq!(
            fixture
                .tool
                .dispatch(&context(), wrong)
                .await
                .unwrap_err()
                .code,
            ErrorCode::WrongTarget
        );
        let mut wrong = request.clone();
        wrong.command = Command::ReadFile {
            worktree: WorktreeId::new(),
            path: "notes.txt".into(),
        };
        assert_eq!(
            fixture
                .tool
                .dispatch(&context(), wrong)
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let mut wrong = request.clone();
        wrong.plugin.as_mut().unwrap().package.digest = "a".repeat(64);
        assert_eq!(
            fixture
                .tool
                .dispatch(&context(), wrong)
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let mut wrong = request;
        wrong.plugin.as_mut().unwrap().turn = Some(TurnId::new());
        assert!(fixture.tool.dispatch(&context(), wrong).await.is_err());

        fixture
            .client
            .execute(fixture.client.prepare(Command::SetPluginEnabled {
                name: "read-fixture".into(),
                expected_revision: 1,
                enabled: false,
            }))
            .await
            .unwrap();
        let before = fixture.node.ingress.plugin_check_count();
        let output = fixture.read(arguments).await;
        let error: Fault = serde_json::from_value(output["error"].clone()).unwrap();
        assert_eq!(error.code, ErrorCode::NotConfigured);
        assert_eq!(fixture.node.ingress.plugin_check_count() - before, 1);
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("source/notes.txt")).unwrap(),
            "Read once\n"
        );
        fixture.close().await;
    }
}
