use std::{path::Path, sync::Arc, time::Duration};

use sailry_client::Client;
use sailry_link::{Identity, Link, Local, NetworkScope, Response};
use sailry_protocol::*;
use tokio::sync::mpsc;

use super::super::{CAPACITY, Job, Services, Store, mutations};

mod entries;
mod outcomes;
mod projects;
mod removal;
mod streaming;
mod trash;
mod upload;

struct Fixture {
    profile: crate::Profile,
    directory: tempfile::TempDir,
    store: Store,
    client: Client,
    worktree: WorktreeId,
    identity: Identity,
}

impl Fixture {
    async fn start(
        execute: impl Fn(&mutations::Roots, &Request) -> Response + Send + 'static,
    ) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let directory =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        let profile = crate::Profile::acquire(&directory.path().join("node-profile")).unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let identity = Identity::open(directory.path()).unwrap();
        let id = identity.id();
        let store = Store::start_ready(
            directory.path().join("node.sqlite3"),
            id,
            Some(profile.path.clone()),
            Services {
                catalog: Default::default(),
                authorization: Default::default(),
                skills: Default::default(),
                environment: Arc::new(crate::process::Environment::ambient()),
                bundled: false,
            },
            execute,
        )
        .await
        .unwrap();
        let client = Client::new(Arc::new(Local::new(id, id, store.ingress.clone())));
        client
            .execute(client.prepare(Command::RegisterProject {
                name: "Project".into(),
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
        Self {
            profile,
            directory,
            store,
            client,
            worktree: snapshot.worktrees[0].id,
            identity,
        }
    }

    fn write(&self, path: &str, text: &str) -> Request {
        self.client.prepare(Command::WriteFile {
            worktree: self.worktree,
            path: path.into(),
            text: text.into(),
            expected_revision: None,
        })
    }
}

struct Gate {
    entered: mpsc::UnboundedReceiver<()>,
    release: std::sync::mpsc::Sender<()>,
}

impl Drop for Gate {
    fn drop(&mut self) {
        let _ = self.release.send(());
    }
}

async fn blocked() -> (Fixture, Gate) {
    let (entered, waiting) = mpsc::unbounded_channel();
    let (release, released) = std::sync::mpsc::channel();
    let fixture = Fixture::start(move |root, request| {
        if matches!(&request.command, Command::WriteFile { path, .. } if path == "blocked") {
            let _ = entered.send(());
            released.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        mutations::execute(root, request)
    })
    .await;
    (
        fixture,
        Gate {
            entered: waiting,
            release,
        },
    )
}

#[tokio::test]
async fn shutdown_drains() {
    let (fixture, mut gate) = blocked().await;
    let first = fixture.write("blocked", "first");
    let admission = fixture.client.dispatch(first.clone()).await.unwrap();
    assert!(admission.receipt.durable);
    gate.entered.recv().await.unwrap();
    let db = rusqlite::Connection::open(fixture.directory.path().join("node.sqlite3")).unwrap();
    let state: String = db
        .query_row(
            "SELECT status FROM requests WHERE id=?1",
            [first.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "admitted");
    drop(db);
    tokio::time::timeout(Duration::from_secs(2), async {
        assert_eq!(
            fixture
                .client
                .execute(first.clone())
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
        fixture
            .client
            .execute(fixture.client.prepare(Command::Snapshot))
            .await
            .unwrap();
        fixture
            .client
            .execute(fixture.client.prepare(Command::ListDirectory {
                worktree: fixture.worktree,
                path: String::new(),
                after: None,
            }))
            .await
            .unwrap();
        fixture
            .client
            .execute(fixture.client.prepare(Command::SetDefaults {
                expected_revision: 0,
                config: SessionConfig {
                    assistant: None,
                    resource: None,
                    provider: ProviderId::new(),
                    model: "model".into(),
                    effort: Effort::High,
                    mode: sailry_protocol::WorkMode::Code,
                    permission: sailry_protocol::Permission::Ask,
                    credential: None,
                },
            }))
            .await
            .unwrap();
    })
    .await
    .expect("storage must not wait for physical file publication");
    let second = fixture
        .client
        .dispatch(fixture.write("blocked", "second"))
        .await
        .unwrap();
    drop(admission);
    fixture.store.stop_admission();
    let stopping = tokio::spawn(fixture.store.shutdown());
    assert!(!stopping.is_finished());
    gate.release.send(()).unwrap();
    gate.entered.recv().await.unwrap();
    gate.release.send(()).unwrap();
    assert_eq!(
        second.completion.await.unwrap().unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    tokio::time::timeout(Duration::from_secs(2), stopping)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(fixture.directory.path().join("project/blocked")).unwrap(),
        "first"
    );
    let store = Store::start_ready(
        fixture.directory.path().join("node.sqlite3"),
        first.target,
        Some(fixture.profile.path.clone()),
        Services {
            catalog: Default::default(),
            authorization: Default::default(),
            skills: Default::default(),
            environment: Arc::new(crate::process::Environment::ambient()),
            bundled: false,
        },
        mutations::execute,
    )
    .await
    .unwrap();
    let client = Client::new(Arc::new(Local::new(
        first.target,
        first.target,
        store.ingress.clone(),
    )));
    assert!(matches!(
        client.execute(first).await.unwrap(),
        Output::FileWritten(_)
    ));
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn capacity_includes_running() {
    let (fixture, mut gate) = blocked().await;
    let first = fixture
        .client
        .dispatch(fixture.write("blocked", "first"))
        .await
        .unwrap();
    gate.entered.recv().await.unwrap();
    let mut queued = Vec::new();
    for index in 1..CAPACITY {
        queued.push(
            fixture
                .client
                .dispatch(fixture.write(&format!("file-{index}"), "file"))
                .await
                .unwrap(),
        );
    }
    let rejected = fixture.write("rejected", "not written");
    assert_eq!(
        fixture
            .client
            .execute(rejected.clone())
            .await
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    gate.release.send(()).unwrap();
    first.completion.await.unwrap().unwrap();
    for admission in queued {
        admission.completion.await.unwrap().unwrap();
    }
    assert_eq!(
        fixture.client.execute(rejected).await.unwrap_err().code,
        ErrorCode::Busy
    );
    assert!(!fixture.directory.path().join("project/rejected").exists());
    fixture.store.shutdown().await.unwrap();
}

#[tokio::test]
async fn ingress_rejects_overflow() {
    assert_eq!(CAPACITY, 128);
    let fixture = Fixture::start(mutations::execute).await;
    let (entered, waiting) = mpsc::unbounded_channel();
    let (release, released) = std::sync::mpsc::channel();
    let mut gate = Gate {
        entered: waiting,
        release,
    };
    fixture
        .store
        .ingress
        .sender
        .send(Job::Barrier {
            entered,
            release: released,
        })
        .await
        .unwrap();
    gate.entered.recv().await.unwrap();

    let client = Arc::new(Client::new(Arc::new(Local::new(
        fixture.identity.id(),
        fixture.identity.id(),
        fixture.store.ingress.clone(),
    ))));
    let mut queued = Vec::with_capacity(CAPACITY);
    for index in 0..CAPACITY {
        let client = client.clone();
        let request = client.prepare(Command::WriteFile {
            worktree: fixture.worktree,
            path: format!("queued-{index}"),
            text: "queued".into(),
            expected_revision: None,
        });
        queued.push(tokio::spawn(async move { client.dispatch(request).await }));
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        while fixture.store.ingress.sender.capacity() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    let overflow = client.prepare(Command::WriteFile {
        worktree: fixture.worktree,
        path: "overflow".into(),
        text: "not written".into(),
        expected_revision: None,
    });
    let error = match client.dispatch(overflow).await {
        Ok(_) => panic!("overflow request was accepted"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::Busy);

    gate.release.send(()).unwrap();
    for task in queued {
        task.await
            .unwrap()
            .unwrap()
            .completion
            .await
            .unwrap()
            .unwrap();
    }
    assert!(!fixture.directory.path().join("project/overflow").exists());
    fixture.store.shutdown().await.unwrap();
}

#[tokio::test]
async fn remote_stays_responsive() {
    let (fixture, mut gate) = blocked().await;
    let first = fixture
        .client
        .dispatch(fixture.write("blocked", "first"))
        .await
        .unwrap();
    gate.entered.recv().await.unwrap();
    let (network, controller, queued) = tokio::time::timeout(Duration::from_secs(5), async {
        let network = Link::bind(
            &fixture.identity,
            NetworkScope::default(),
            fixture.store.ingress.clone(),
            fixture.store.ingress.clone(),
        )
        .await
        .unwrap();
        let controller = Link::controller(
            fixture.directory.path().join("controller"),
            NetworkScope::default(),
        )
        .await
        .unwrap();
        let invitation = network.handle().invite().unwrap();
        let address = controller.handle().pair(invitation.ticket()).await.unwrap();
        let remote = Client::new(controller.handle().remote(address));
        remote
            .execute(remote.prepare(Command::Snapshot))
            .await
            .unwrap();
        let request = remote.prepare(Command::WriteFile {
            worktree: fixture.worktree,
            path: "remote".into(),
            text: "detached observer".into(),
            expected_revision: None,
        });
        let queued = remote.dispatch(request).await.unwrap();
        assert!(queued.receipt.durable);
        (network, controller, queued)
    })
    .await
    .expect("pairing and admission must remain responsive");
    drop(queued);
    controller.close().await.unwrap();
    gate.release.send(()).unwrap();
    first.completion.await.unwrap().unwrap();
    fixture.store.stop_admission();
    network.close().await.unwrap();
    fixture.store.shutdown().await.unwrap();
    assert_eq!(
        std::fs::read_to_string(fixture.directory.path().join("project/remote")).unwrap(),
        "detached observer"
    );
}

#[tokio::test]
async fn reports_panics() {
    let fixture = Fixture::start(|root, request| {
        let result = mutations::execute(root, request);
        if matches!(&request.command, Command::WriteFile { path, .. } if path == "panic") {
            panic!("injected failure after publication");
        }
        result
    })
    .await;
    let request = fixture.write("panic", "published");
    assert_eq!(
        fixture
            .client
            .execute(request.clone())
            .await
            .unwrap_err()
            .code,
        ErrorCode::OutcomeUnknown
    );
    assert_eq!(
        fixture.client.execute(request).await.unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    fixture
        .client
        .execute(fixture.write("after", "worker remains available"))
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(fixture.directory.path().join("project/panic")).unwrap(),
        "published"
    );
    fixture.store.shutdown().await.unwrap();
}
