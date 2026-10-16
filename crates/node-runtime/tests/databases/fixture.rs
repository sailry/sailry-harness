use super::*;

/// One execution profile and authenticated controller per engine. Restarting
/// preserves their identities and uses the ordinary durable Node lifecycle.
pub(super) struct Fixture {
    node: Option<Node>,
    controller: Link,
    directory: tempfile::TempDir,
}

impl Fixture {
    pub async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        Self {
            directory,
            node: Some(node),
            controller,
        }
    }

    pub fn id(&self) -> NodeId {
        self.node.as_ref().unwrap().id()
    }

    pub fn client(&self, remote: bool) -> Client {
        let node = self.node.as_ref().unwrap();
        Client::new(if remote {
            self.controller.handle().remote(node.link().address())
        } else {
            node.local()
        })
    }

    pub async fn restart(&mut self) {
        self.node.take().unwrap().shutdown().await.unwrap();
        self.node = Some(
            Node::start_empty(self.directory.path().join("node"))
                .await
                .unwrap(),
        );
    }

    pub async fn close(mut self) {
        self.controller.close().await.unwrap();
        self.node.take().unwrap().shutdown().await.unwrap();
    }
}
