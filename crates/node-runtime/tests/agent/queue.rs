use super::*;
use sailry_client::conversation::Projection;

#[path = "queue/persistence.rs"]
mod persistence;

struct Fixture {
    directory: tempfile::TempDir,
    node: Node,
    controller: Link,
    client: Client,
    session: Session,
    provider: Provider,
    server: Server,
    remote: bool,
}

impl Fixture {
    async fn start(remote: bool, slow: bool) -> Self {
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
        let transport = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let client = Client::new(transport);
        let server = Server::start(slow).await;
        let (session, provider) = configured(&client, &server.endpoint, &root).await;
        Self {
            directory,
            node,
            controller,
            client,
            session,
            provider,
            server,
            remote,
        }
    }

    async fn push(&self, message: &str) -> QueuedTurn {
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

    async fn queue(&self) -> Queue {
        history(&self.client, self.session.id).await.queue
    }

    async fn enqueue(&self, message: &str) -> QueuedTurn {
        let Output::QueuedTurn(turn) = self
            .client
            .execute(self.client.prepare(Command::QueueTurn {
                session: self.session.id,
                expected_revision: self.session.revision,
                message: message.into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        self.client
            .execute(
                self.client
                    .prepare(Command::StartQueuedTurn { turn: turn.id }),
            )
            .await
            .unwrap();
        turn
    }

    async fn pause(&self, paused: bool) {
        self.client
            .execute(self.client.prepare(Command::SetQueuePaused {
                session: self.session.id,
                expected_revision: self.queue().await.revision,
                paused,
            }))
            .await
            .unwrap();
    }

    async fn restart(mut self) -> Self {
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

    async fn close(self) {
        self.node.shutdown().await.unwrap();
        self.controller.close().await.unwrap();
    }
}

pub(super) async fn read(client: &Client, turn: TurnId) -> QueuedMessage {
    let Output::QueuedMessage(message) = client
        .execute(client.prepare(Command::ReadQueuedTurn { turn }))
        .await
        .unwrap()
    else {
        panic!("queued message expected")
    };
    message
}

async fn projected(
    stream: &mut dyn sailry_link::Subscription,
    projection: &mut Projection,
    generation: u64,
    revision: u64,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            projection
                .apply(generation, stream.next().await.unwrap())
                .unwrap();
            if projection
                .snapshot()
                .is_some_and(|snapshot| snapshot.page.queue.revision == revision)
            {
                break;
            }
        }
    })
    .await
    .expect("queue projection deadline");
}

fn prompt(request: &serde_json::Value) -> &str {
    request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|message| message["role"] == "user")
        .unwrap()["content"]
        .as_str()
        .unwrap()
}

#[tokio::test]
async fn edits_survive_restart() {
    for remote in [false, true] {
        let fixture = Fixture::start(remote, false).await;
        fixture.pause(true).await;
        let mut stream = fixture
            .client
            .subscribe_conversation(fixture.session.id)
            .await
            .unwrap();
        let mut projection = Projection::new(fixture.node.id(), fixture.session.id, 1);
        let first = fixture.enqueue("first").await;
        let middle = fixture.enqueue("middle").await;
        let last = fixture.enqueue("last").await;
        let queue = fixture.queue().await;
        projected(stream.as_mut(), &mut projection, 1, queue.revision).await;
        assert_eq!(projection.snapshot().unwrap().page.queue, queue);
        let mut config = fixture.session.config.clone();
        config.model = "fixture-b".into();
        config.effort = Effort::Low;
        fixture
            .client
            .execute(fixture.client.prepare(Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: 1,
                config,
            }))
            .await
            .unwrap();
        let message = format!("edited {}", "中文🙂\n".repeat(1000));
        let edit = fixture.client.prepare(Command::EditQueuedTurn {
            turn: middle.id,
            expected_revision: 1,
            message: message.clone().into(),
        });
        let edited = fixture.client.execute(edit.clone()).await.unwrap();
        let queued = read(&fixture.client, middle.id).await;
        assert_eq!(queued.message.text, message);
        assert_eq!(queued.revision, 2);
        assert_eq!(queued.turn, middle);
        let queue = fixture.queue().await;
        assert!(queue.items[1].truncated);
        assert_eq!(queue.items[1].preview.chars().count(), 256);
        fixture
            .client
            .execute(fixture.client.prepare(Command::MoveQueuedTurn {
                session: fixture.session.id,
                expected_revision: queue.revision,
                turn: last.id,
                before: Some(first.id),
            }))
            .await
            .unwrap();
        fixture
            .client
            .execute(fixture.client.prepare(Command::RemoveQueuedTurn {
                turn: first.id,
                expected_revision: 1,
            }))
            .await
            .unwrap();
        let queue = fixture.queue().await;
        assert_eq!(
            queue.items.iter().map(|item| item.turn).collect::<Vec<_>>(),
            vec![last.id, middle.id]
        );
        assert_eq!(fixture.client.execute(edit.clone()).await.unwrap(), edited);
        assert_eq!(fixture.queue().await, queue);
        projected(stream.as_mut(), &mut projection, 1, queue.revision).await;
        assert_eq!(projection.snapshot().unwrap().page.queue, queue);
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        let database =
            rusqlite::Connection::open(fixture.directory.path().join("node/storage/node.sqlite3"))
                .unwrap();
        let original: Vec<u8> = database
            .query_row(
                "SELECT body FROM requests WHERE id=?1",
                [middle.request.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Request>(&original)
                .unwrap()
                .command,
            Command::QueueTurn {
                session: fixture.session.id,
                expected_revision: 1,
                message: "middle".into(),
            }
        );
        assert_eq!(
            database
                .query_row("SELECT count(*) FROM agent_events", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        drop(database);
        drop(stream);
        let fixture = fixture.restart().await;
        assert_eq!(fixture.queue().await, queue);
        assert_eq!(read(&fixture.client, middle.id).await, queued);
        let mut stream = fixture
            .client
            .subscribe_conversation(fixture.session.id)
            .await
            .unwrap();
        projection.reconnect(2).unwrap();
        projected(stream.as_mut(), &mut projection, 2, queue.revision).await;
        assert_eq!(projection.snapshot().unwrap().page.queue, queue);
        fixture.pause(false).await;
        let page = finished(&fixture.client, fixture.session.id, middle.id).await;
        assert!(page.queue.items.is_empty());
        assert!(
            page.runs
                .iter()
                .filter(|run| run.turn != first.id)
                .all(|run| run.status == Status::Completed)
        );
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == first.id)
                .unwrap()
                .status,
            Status::Cancelled
        );
        assert!(!page.entries.iter().any(|entry| entry.turn == first.id));
        let requests = fixture.server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(prompt(&requests[0]), "last");
        assert_eq!(prompt(&requests[1]), message);
        assert!(requests.iter().all(
            |request| request["model"] == "fixture-a" && request["reasoning_effort"] == "high"
        ));
        assert_eq!(fixture.client.execute(edit).await.unwrap(), edited);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
        drop(stream);
        fixture.close().await;
    }
}

#[tokio::test]
async fn send_now() {
    for remote in [false, true] {
        let fixture = Fixture::start(remote, true).await;
        let active = fixture.push("active").await;
        fixture.server.wait_count(1).await;
        let Output::Session(other) = fixture
            .client
            .execute(fixture.client.prepare(Command::CreateSession {
                project: fixture.session.project,
                worktree: None,
                config: Some(fixture.session.config.clone()),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let Output::QueuedTurn(other_turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::SubmitTurn {
                session: other.id,
                expected_revision: 1,
                message: "other session".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        fixture.server.wait_count(2).await;
        let quick = Server::start(false).await;
        let mut provider = fixture.provider.clone();
        provider.endpoint = quick.endpoint.clone();
        fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                provider,
                expected_revision: 1,
            }))
            .await
            .unwrap();
        let waiting = fixture.push("waiting").await;
        let selected = fixture.push("selected").await;
        let send = fixture.client.prepare(Command::SendQueuedTurn {
            turn: selected.id,
            expected_revision: 1,
        });
        let result = fixture.client.execute(send.clone()).await.unwrap();
        let page = finished(&fixture.client, fixture.session.id, waiting.id).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == active.id)
                .unwrap()
                .status,
            Status::Cancelled
        );
        assert!(
            page.runs
                .iter()
                .filter(|run| run.turn != active.id)
                .all(|run| run.status == Status::Completed)
        );
        let requests = quick.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(prompt(&requests[0]), "selected");
        assert_eq!(prompt(&requests[1]), "waiting");
        assert!(!page.queue.paused);
        assert_eq!(
            history(&fixture.client, other.id).await.runs[0].status,
            Status::Running
        );
        assert_eq!(fixture.client.execute(send).await.unwrap(), result);
        assert_eq!(quick.requests.lock().unwrap().len(), 2);
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::EditQueuedTurn {
                    turn: selected.id,
                    expected_revision: 1,
                    message: "too late".into(),
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        fixture
            .client
            .execute(fixture.client.prepare(Command::StopTurn {
                turn: other_turn.id,
            }))
            .await
            .unwrap();
        assert_eq!(
            finished(&fixture.client, other.id, other_turn.id)
                .await
                .runs[0]
                .status,
            Status::Cancelled
        );
        fixture.close().await;
    }
}

#[tokio::test]
async fn conflicts_preserve_input() {
    for remote in [false, true] {
        let fixture = Fixture::start(remote, false).await;
        fixture.pause(true).await;
        let turn = fixture.enqueue("original").await;
        let other = Client::new(fixture.node.local());
        let edit = |message: &str| Command::EditQueuedTurn {
            turn: turn.id,
            expected_revision: 1,
            message: message.into(),
        };
        let (left, right) = tokio::join!(
            fixture.client.execute(fixture.client.prepare(edit("left"))),
            other.execute(other.prepare(edit("right")))
        );
        let error = match (left, right) {
            (Err(error), Ok(_)) | (Ok(_), Err(error)) => error,
            _ => panic!("exactly one edit must commit"),
        };
        assert_eq!(error.code, ErrorCode::RevisionConflict);
        let queued = read(&fixture.client, turn.id).await;
        let queue = fixture.queue().await;
        assert_eq!(queued.revision, 2);
        for command in [
            Command::RemoveQueuedTurn {
                turn: turn.id,
                expected_revision: 1,
            },
            Command::SendQueuedTurn {
                turn: turn.id,
                expected_revision: 1,
            },
            Command::SetQueuePaused {
                session: fixture.session.id,
                expected_revision: queue.revision - 1,
                paused: false,
            },
            Command::MoveQueuedTurn {
                session: fixture.session.id,
                expected_revision: queue.revision - 1,
                turn: turn.id,
                before: None,
            },
        ] {
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(command))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::RevisionConflict
            );
        }
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::MoveQueuedTurn {
                    session: fixture.session.id,
                    expected_revision: queue.revision,
                    turn: turn.id,
                    before: Some(TurnId::new()),
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::EditQueuedTurn {
                    turn: turn.id,
                    expected_revision: 2,
                    message: "  \n".into(),
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(fixture.queue().await, queue);
        assert_eq!(read(&other, turn.id).await, queued);
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        fixture.close().await;
    }
}
