use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::*;
use std::path::{Path, PathBuf};

pub struct Fixture {
    pub _temp: tempfile::TempDir,
    pub root: PathBuf,
    pub node: Node,
    pub controller: Link,
    pub worktree: WorktreeId,
}

impl Fixture {
    pub async fn start() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start(temp.path().join("node")).await.unwrap();
        let local = Client::new(node.local());
        let worktree = register(&local, &root).await;
        let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
            .await
            .unwrap();
        controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        Self {
            _temp: temp,
            root,
            node,
            controller,
            worktree,
        }
    }

    pub fn clients(&self) -> [Client; 2] {
        [
            Client::new(self.node.local()),
            Client::new(self.controller.handle().remote(self.node.link().address())),
        ]
    }

    pub async fn close(self) {
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}

pub async fn register(client: &Client, path: &Path) -> WorktreeId {
    let Output::Project(project) = client
        .execute(client.prepare(Command::RegisterProject {
            name: "File fixture".into(),
            path: path.to_str().unwrap().into(),
        }))
        .await
        .unwrap()
    else {
        panic!("project expected")
    };
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    snapshot
        .worktrees
        .iter()
        .find(|tree| tree.project == Some(project.id))
        .unwrap()
        .id
}
