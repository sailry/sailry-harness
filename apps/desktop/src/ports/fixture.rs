use super::*;
use sailry_node_runtime::Node;
use sailry_protocol::Command;

#[path = "../../../../crates/node-runtime/tests/ports/server.rs"]
#[allow(dead_code)]
mod server;

pub(super) struct Fixture {
    _directory: tempfile::TempDir,
    pub runtime: Arc<tokio::runtime::Runtime>,
    pub nodes: [Node; 2],
    pub server: server::Server,
}
impl Fixture {
    pub fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let nodes = ["local", "remote"].map(|name| {
            runtime
                .block_on(Node::start(directory.path().join(name)))
                .unwrap()
        });
        runtime
            .block_on(
                nodes[0]
                    .link()
                    .pair(nodes[1].link().invite().unwrap().ticket()),
            )
            .unwrap();
        for (index, node) in nodes.iter().enumerate().skip(1) {
            let root = directory.path().join(format!("project-{index}"));
            std::fs::create_dir(&root).unwrap();
            let client = Client::new(node.local());
            runtime
                .block_on(client.execute(client.prepare(Command::RegisterProject {
                    name: "Development project".into(),
                    path: root.to_str().unwrap().into(),
                })))
                .unwrap();
        }
        let server = runtime.block_on(server::Server::start());
        Self {
            _directory: directory,
            runtime,
            nodes,
            server,
        }
    }
    pub fn services(&self) -> crate::backend::Services {
        crate::backend::Services {
            runtime: self.runtime.clone(),
            link: self.nodes[0].link(),
            local: self.nodes[0].local(),
            relay_enabled: false,
        }
    }
    pub fn close(self) {
        for node in self.nodes {
            self.runtime.block_on(node.shutdown()).unwrap();
        }
        self.runtime.block_on(self.server.close());
    }
}
