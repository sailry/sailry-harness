use super::*;

pub(crate) struct Fixture {
    pub directory: tempfile::TempDir,
    pub node: Node,
    pub controller: Link,
    pub client: Client,
    pub session: Session,
    pub provider: Provider,
    pub role: role::Profile,
    pub parent: Server,
    pub child: Server,
    remote: bool,
}

impl Fixture {
    pub async fn start(remote: bool, parent: Server, child: Server) -> Self {
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
        let (session, provider, role) = setup(&client, &root, &parent, &child).await;
        Self {
            directory,
            node,
            controller,
            client,
            session,
            provider,
            role,
            parent,
            child,
            remote,
        }
    }

    pub fn root(&self) -> std::path::PathBuf {
        self.directory.path().join("project")
    }

    pub async fn push(&self, message: &str) -> QueuedTurn {
        let Output::QueuedTurn(turn) = self
            .client
            .execute(self.client.prepare(Command::SubmitTurn {
                session: self.session.id,
                expected_revision: self.session.revision,
                message: message.into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        turn
    }

    pub async fn snapshot(&self) -> sailry_protocol::Snapshot {
        let Output::Snapshot(snapshot) = self
            .client
            .execute(self.client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        snapshot
    }

    pub async fn route_parent(&self, endpoint: &str) {
        let mut provider = self
            .snapshot()
            .await
            .providers
            .into_iter()
            .find(|provider| provider.id == self.session.config.provider)
            .unwrap();
        let revision = provider.revision;
        provider.endpoint = endpoint.into();
        self.client
            .execute(self.client.prepare(Command::PutProvider {
                provider,
                expected_revision: revision,
            }))
            .await
            .unwrap();
    }

    pub async fn upload(&self, worktree: WorktreeId, bytes: &[u8]) -> attachment::Attachment {
        let Output::AttachmentUpload(upload) = self
            .client
            .execute(
                self.client
                    .prepare(Command::UploadAttachment(attachment::Spec {
                        worktree,
                        name: "context.txt".into(),
                        media_type: "text/plain".into(),
                        size: bytes.len() as u64,
                        revision: blake3::hash(bytes).to_hex().to_string(),
                    })),
            )
            .await
            .unwrap()
        else {
            panic!("upload expected")
        };
        self.client
            .upload(
                &upload,
                &mut &bytes[..],
                sailry_link::CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap();
        let Output::Attachment(attachment) = self
            .client
            .execute(self.client.prepare(Command::FinishAttachmentUpload {
                worktree,
                stream: upload.stream,
            }))
            .await
            .unwrap()
        else {
            panic!("attachment expected")
        };
        attachment
    }

    pub async fn restart(mut self) -> Self {
        let id = self.node.id();
        self.node.shutdown().await.unwrap();
        self.controller.handle().disconnect(id).await;
        self.node = Node::start(self.directory.path().join("node"))
            .await
            .unwrap();
        self.client = Client::new(if self.remote {
            self.controller.handle().remote(self.node.link().address())
        } else {
            self.node.local()
        });
        self
    }

    pub async fn close(self) {
        self.node.shutdown().await.unwrap();
        self.controller.close().await.unwrap();
    }
}
