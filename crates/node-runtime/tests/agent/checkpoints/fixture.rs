use super::*;
use std::{collections::HashSet, path::PathBuf};

pub(super) struct Fixture {
    pub directory: tempfile::TempDir,
    pub root: PathBuf,
    pub node: Node,
    pub controller: Link,
    pub transport: Arc<dyn Transport>,
    pub client: Client,
    pub server: Server,
    pub session: Session,
    pub turn: TurnId,
    pub history: Page,
    pub files: Vec<checkpoint::File>,
}

impl Fixture {
    pub async fn start(remote: bool, changes: &[(&str, Option<&str>, &str)]) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let mut initialized = HashSet::new();
        for &(path, before, _) in changes {
            if initialized.insert(path)
                && let Some(before) = before
            {
                std::fs::write(root.join(path), before).unwrap();
            }
        }
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
        let transport = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let client = Client::new(transport.clone());
        let server = Server::tools(changes.iter().map(|&(path, before, after)| (
            plugin_tool("files", "write_file"), json!({"path": path, "text": after, "expected_revision": before.map(|text| blake3::hash(text.as_bytes()).to_hex().to_string())}),
        )).collect()).await;
        let session = approvals::prepare(&client, &server, &root).await;
        let mut config = session.config.clone();
        config.permission = Permission::Full;
        let Output::Session(session) = client
            .execute(client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Make file changes".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let history = finished(&client, session.id, turn.id).await;
        assert_eq!(history.runs[0].status, Status::Completed);
        let files = list(&client, session.id, turn.id, None, 100).await.files;
        assert_eq!(files.len(), changes.len());
        Self {
            directory,
            root,
            node,
            controller,
            transport,
            client,
            server,
            session,
            turn: turn.id,
            history,
            files,
        }
    }

    pub fn restore(&self, file: &checkpoint::File) -> Request {
        self.client.prepare(Command::RestoreFileCheckpoint {
            session: self.session.id,
            checkpoint: file.id,
            worktree: self.session.worktree,
        })
    }

    pub fn database(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.directory.path().join("node/storage/node.sqlite3")).unwrap()
    }

    pub fn counts(&self) -> (i64, i64) {
        self.database().query_row("SELECT (SELECT count(*) FROM agent_events),(SELECT count(*) FROM file_checkpoints)", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap()
    }
}
