use super::{creation::init, discovery::adopt, *};
use git2::{BranchType, Repository};
use sailry_client::{Apply, Projection};
use sailry_link::{Link, NetworkScope};
use std::{path::Path, time::Duration};

struct Fixture {
    temp: tempfile::TempDir,
    repository: Repository,
    node: Node,
    controller: Link,
    local: Client,
    remote: Client,
    project: Project,
}

impl Fixture {
    async fn start() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let repository = init(&temp.path().join("repository"));
        let node = Node::start(temp.path().join("node")).await.unwrap();
        let local = Client::new(node.local());
        let project = register(&local, repository.workdir().unwrap()).await;
        let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
            .await
            .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let remote = Client::new(controller.handle().remote(address));
        Self {
            temp,
            repository,
            node,
            controller,
            local,
            remote,
            project,
        }
    }

    async fn create(&self, name: &str, client: &Client) -> (Worktree, Command) {
        let path = self.temp.path().join(name);
        self.repository.worktree(name, &path, None).unwrap();
        let tree = adopt(client, self.project.id, &path).await;
        let command = Command::RemoveWorktree {
            worktree: tree.id,
            expected_head: self
                .repository
                .head()
                .unwrap()
                .target()
                .unwrap()
                .to_string(),
            expected_branch: name.into(),
        };
        (tree, command)
    }

    async fn close(self) {
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn local_and_remote_removal() {
    let fixture = Fixture::start().await;
    let main = snapshot(&fixture.local).await.worktrees[0].clone();
    for (sequence, client) in [&fixture.local, &fixture.remote].into_iter().enumerate() {
        let name = format!("中文-{sequence}");
        let (tree, command) = fixture.create(&name, client).await;
        let mut events = client.subscribe().await.unwrap();
        let mut projection = Projection::new(fixture.node.id(), 1);
        projection.apply(1, events.next().await.unwrap()).unwrap();
        let transport = if sequence == 0 {
            fixture.node.local()
        } else {
            fixture
                .controller
                .handle()
                .remote(fixture.node.link().address())
        };
        let mut watch = transport.subscribe(Topic::Files(tree.id)).await.unwrap();
        watch.next().await.unwrap();
        let Output::FileDownload(download) = client
            .execute(client.prepare(Command::DownloadFile {
                worktree: tree.id,
                path: "file".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("download expected")
        };
        let mut uploads = Vec::new();
        let mut active = None;
        for phase in ["pending", "active", "staged"] {
            let Output::FileUpload(upload) = client
                .execute(client.prepare(Command::UploadFile(FileUploadSpec {
                    worktree: tree.id,
                    path: phase.into(),
                    size: 7,
                    revision: blake3::hash(b"content").to_hex().to_string(),
                    expected_revision: None,
                })))
                .await
                .unwrap()
            else {
                panic!("upload expected")
            };
            if phase == "active" {
                active = Some(client.open(upload.stream).await.unwrap());
            }
            if phase == "staged" {
                client
                    .upload(
                        &upload,
                        &mut &b"content"[..],
                        sailry_link::CancellationToken::new(),
                        |_| {},
                    )
                    .await
                    .unwrap();
            }
            uploads.push(upload);
        }
        let request = client.prepare(command);
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let expected = Output::WorktreeRemoved { id: tree.id };
        assert_eq!(admission.completion.await.unwrap().unwrap(), expected);
        assert!(client.open(download.stream).await.is_err());
        for upload in uploads {
            assert!(client.open(upload.stream).await.is_err());
            assert_eq!(
                client
                    .execute(client.prepare(Command::FinishFileUpload {
                        worktree: tree.id,
                        path: upload.spec.path,
                        stream: upload.stream
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
        }
        if let Some(mut stream) = active {
            use tokio::io::AsyncReadExt;
            let result = tokio::time::timeout(Duration::from_secs(3), stream.read(&mut [0]))
                .await
                .unwrap();
            assert!(result.is_err() || result.unwrap() == 0);
        }
        assert!(!Path::new(&tree.path).exists());
        assert!(fixture.repository.find_worktree(&name).is_err());
        assert!(
            fixture
                .repository
                .find_branch(&name, BranchType::Local)
                .is_ok()
        );
        let update = tokio::time::timeout(Duration::from_secs(5), events.next())
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(&update, Update::Event(event) if event.event == Event::WorktreeRemoved { id: tree.id })
        );
        assert_eq!(projection.apply(1, update.clone()).unwrap(), Apply::Applied);
        assert_eq!(projection.apply(1, update).unwrap(), Apply::Ignored);
        assert_eq!(projection.snapshot().unwrap(), &snapshot(client).await);
        assert!(
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if watch.next().await.is_err() {
                        break;
                    }
                }
            })
            .await
            .is_ok()
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::ListDirectory {
                    worktree: tree.id,
                    path: String::new(),
                    after: None,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        std::fs::create_dir(&tree.path).unwrap();
        std::fs::write(Path::new(&tree.path).join("sentinel"), "replacement").unwrap();
        assert_eq!(client.execute(request).await.unwrap(), expected);
        assert_eq!(
            std::fs::read(Path::new(&tree.path).join("sentinel")).unwrap(),
            b"replacement"
        );
        assert!(snapshot(client).await.worktrees.contains(&main));
    }
    assert_eq!(
        std::fs::read(Path::new(&main.path).join("file")).unwrap(),
        b"base"
    );
    fixture.close().await;
}

async fn rejects(client: &Client, command: &Command, code: ErrorCode) {
    assert_eq!(
        client
            .execute(client.prepare(command.clone()))
            .await
            .unwrap_err()
            .code,
        code
    );
}

#[tokio::test]
async fn preserves_protected_worktrees() {
    let fixture = Fixture::start().await;
    for (sequence, client) in [&fixture.local, &fixture.remote].into_iter().enumerate() {
        let name = format!("protected-{sequence}");
        let (tree, command) = fixture.create(&name, client).await;
        let root = Path::new(&tree.path);
        let repository = Repository::open(root).unwrap();
        let native = fixture.repository.find_worktree(&name).unwrap();
        let before = snapshot(client).await;
        native.lock(Some("Fixture lock")).unwrap();
        rejects(client, &command, ErrorCode::Busy).await;
        native.unlock().unwrap();
        for filename in ["index.lock", "HEAD.lock"] {
            let lock = repository.path().join(filename);
            std::fs::write(&lock, "fixture lock").unwrap();
            rejects(client, &command, ErrorCode::Busy).await;
            assert_eq!(std::fs::read(&lock).unwrap(), b"fixture lock");
            std::fs::remove_file(lock).unwrap();
        }
        std::fs::write(root.join("file"), "draft").unwrap();
        rejects(client, &command, ErrorCode::Conflict).await;
        assert_eq!(std::fs::read(root.join("file")).unwrap(), b"draft");
        let mut index = repository.index().unwrap();
        index.add_path(Path::new("file")).unwrap();
        index.write().unwrap();
        rejects(client, &command, ErrorCode::Conflict).await;
        std::fs::write(root.join("file"), "base").unwrap();
        index.add_path(Path::new("file")).unwrap();
        index.write().unwrap();
        drop(index);
        std::fs::write(root.join("untracked"), "preserve").unwrap();
        rejects(client, &command, ErrorCode::Conflict).await;
        std::fs::remove_file(root.join("untracked")).unwrap();
        let head = repository.head().unwrap().target().unwrap();
        repository.set_head_detached(head).unwrap();
        rejects(client, &command, ErrorCode::RevisionConflict).await;
        repository.set_head(&format!("refs/heads/{name}")).unwrap();
        std::fs::write(repository.path().join("MERGE_HEAD"), head.to_string()).unwrap();
        rejects(client, &command, ErrorCode::Conflict).await;
        std::fs::remove_file(repository.path().join("MERGE_HEAD")).unwrap();
        let mut stale = command.clone();
        if let Command::RemoveWorktree { expected_head, .. } = &mut stale {
            *expected_head = git2::Oid::ZERO_SHA1.to_string();
        }
        rejects(client, &stale, ErrorCode::RevisionConflict).await;
        assert_eq!(snapshot(client).await, before);
        client
            .execute(client.prepare(Command::CreateSession {
                project: tree.project,
                worktree: Some(tree.id),
                config: Some(config()),
            }))
            .await
            .unwrap();
        rejects(client, &command, ErrorCode::Conflict).await;
        assert_eq!(repository.head().unwrap().target(), Some(head));
        assert_eq!(std::fs::read(root.join("file")).unwrap(), b"base");
        native.validate().unwrap();
        let main = before.worktrees.iter().find(|tree| tree.main).unwrap();
        let mut main_command = command.clone();
        if let Command::RemoveWorktree { worktree, .. } = &mut main_command {
            *worktree = main.id;
        }
        rejects(client, &main_command, ErrorCode::Conflict).await;
        if let Command::RemoveWorktree { worktree, .. } = &mut main_command {
            *worktree = WorktreeId::new();
        }
        rejects(client, &main_command, ErrorCode::NotFound).await;
    }
    fixture.close().await;
}

#[tokio::test]
async fn preserves_nested_resources() {
    let fixture = Fixture::start().await;
    for (sequence, client) in [&fixture.local, &fixture.remote].into_iter().enumerate() {
        let name = format!("nested-{sequence}");
        let (tree, command) = fixture.create(&name, client).await;
        let child = Path::new(&tree.path).join("ignored");
        std::fs::create_dir(&child).unwrap();
        std::fs::write(fixture.repository.path().join("info/exclude"), "ignored/\n").unwrap();
        register(client, &child).await;
        rejects(client, &command, ErrorCode::Conflict).await;
        assert!(child.is_dir());
        fixture
            .repository
            .find_worktree(&name)
            .unwrap()
            .validate()
            .unwrap();

        let nested_node = Node::start(child.join("node")).await.unwrap();
        let nested_client = Client::new(nested_node.local());
        let project = register(&nested_client, fixture.repository.workdir().unwrap()).await;
        let owned = adopt(&nested_client, project.id, Path::new(&tree.path)).await;
        let mut owned_command = command.clone();
        if let Command::RemoveWorktree { worktree, .. } = &mut owned_command {
            *worktree = owned.id;
        }
        rejects(&nested_client, &owned_command, ErrorCode::Conflict).await;
        nested_node.shutdown().await.unwrap();

        let (unregistered, command) = fixture
            .create(&format!("unregistered-{sequence}"), client)
            .await;
        let nested = Path::new(&unregistered.path).join("ignored");
        fixture
            .repository
            .worktree(&format!("native-{sequence}"), &nested, None)
            .unwrap();
        rejects(client, &command, ErrorCode::Conflict).await;
        assert!(nested.join(".git").is_file());
    }
    fixture.close().await;
}

#[tokio::test]
async fn lost_result_is_not_replayed() {
    for remote in [false, true] {
        let fixture = Fixture::start().await;
        let client = if remote {
            &fixture.remote
        } else {
            &fixture.local
        };
        let (tree, command) = fixture.create("lost-result", client).await;
        let before = snapshot(client).await;
        let profile = fixture.node.profile().to_owned();
        let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
        db.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
        let request = client.prepare(command);
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert!(!Path::new(&tree.path).exists());
        assert_eq!(snapshot(client).await, before);
        db.execute_batch("DROP TRIGGER lose_result").unwrap();
        drop(db);
        fixture.node.shutdown().await.unwrap();
        std::fs::create_dir(&tree.path).unwrap();
        std::fs::write(Path::new(&tree.path).join("sentinel"), "replacement").unwrap();
        let node = Node::start(&profile).await.unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(snapshot(&client).await, before);
        assert_eq!(
            std::fs::read(Path::new(&tree.path).join("sentinel")).unwrap(),
            b"replacement"
        );
        assert!(
            fixture
                .repository
                .find_branch("lost-result", BranchType::Local)
                .is_ok()
        );
        fixture.controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
