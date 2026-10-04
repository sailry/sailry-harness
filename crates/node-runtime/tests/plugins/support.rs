use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{Command, Output, WorktreeId, plugin::Info};
use std::{fs, path::Path};

pub(crate) fn package(root: &Path, version: &str) {
    fs::create_dir_all(root.join("skills/analysis/references")).unwrap();
    fs::create_dir_all(root.join(sailry_protocol::plugin::NAMESPACE)).unwrap();
    fs::write(
        root.join("plugin.json"),
        serde_json::json!({
            "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name": "example", "version": version, "description": "分析数据 🙂",
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        root.join("skills/analysis/SKILL.md"),
        "---\nname: analysis\ndescription: Analyze project data\n---\nRead references/guide.md\n",
    )
    .unwrap();
    fs::write(root.join("skills/analysis/references/guide.md"), version).unwrap();
}

pub(crate) async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

pub(crate) fn info(output: Output) -> Info {
    let Output::Plugin(info) = output else {
        panic!("plugin information expected")
    };
    info
}

pub(crate) async fn fixture(remote: bool) -> (tempfile::TempDir, Node, Link, Client, WorktreeId) {
    let directory = tempfile::tempdir().unwrap();
    let node = Node::start(directory.path().join("node")).await.unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
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
    let source = directory.path().join("source");
    package(&source.join("package"), "1.0.0");
    execute(
        &client,
        Command::RegisterProject {
            name: "Plugin fixture".into(),
            path: source.to_str().unwrap().into(),
        },
    )
    .await;
    let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    let worktree = snapshot.worktrees[0].id;
    (directory, node, controller, client, worktree)
}

pub(crate) fn install(worktree: WorktreeId, expected_revision: u64) -> Command {
    Command::InstallPlugin {
        worktree,
        path: "package".into(),
        name: "example".into(),
        expected_revision,
    }
}
