#![cfg(unix)]
use git2::{Repository, Signature};
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::*;
use std::path::Path;

async fn setup(client: &Client, root: &Path) -> WorktreeId {
    client
        .execute(client.prepare(Command::RegisterProject {
            name: "Index fixture".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap();
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!()
    };
    snapshot.worktrees[0].id
}

async fn change(
    client: &Client,
    id: WorktreeId,
    operation: GitIndexChange,
    paths: &[&str],
) -> Request {
    let Output::GitStatus(status) = client
        .execute(client.prepare(Command::InspectGit { worktree: id }))
        .await
        .unwrap()
    else {
        panic!()
    };
    client.prepare(Command::UpdateGitIndex {
        worktree: id,
        operation,
        paths: paths.iter().map(|path| (*path).into()).collect(),
        expected_index: status.index_revision.unwrap(),
        expected_head: status.head,
    })
}

fn fixture(root: &Path) -> Repository {
    let repository = Repository::init(root).unwrap();
    std::fs::write(root.join("tracked"), "base\n").unwrap();
    std::fs::write(root.join("other"), "base other\n").unwrap();
    let mut index = repository.index().unwrap();
    index
        .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
    let author = Signature::now("Fixture", "fixture@example.invalid").unwrap();
    repository
        .commit(Some("HEAD"), &author, &author, "Initial", &tree, &[])
        .unwrap();
    drop(tree);
    drop(index);
    repository
}

fn staged(repository: &Repository, path: &str) -> String {
    let entry = disk_index(repository).get_path(Path::new(path), 0).unwrap();
    String::from_utf8(repository.find_blob(entry.id).unwrap().content().to_vec()).unwrap()
}

fn disk_index(repository: &Repository) -> git2::Index {
    git2::Index::open(&repository.path().join("index")).unwrap()
}

#[tokio::test]
async fn shares_scoped_index_updates() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let repository = fixture(&root);
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let id = setup(&local, &root).await;
    let controller = Link::controller(temp.path().join("client"), NetworkScope::default())
        .await
        .unwrap();
    let invite = node.link().invite().unwrap();
    let address = controller.handle().pair(invite.ticket()).await.unwrap();
    let remote = Client::new(controller.handle().remote(address));
    std::fs::write(root.join("other"), "external staged\n").unwrap();
    let mut external = repository.index().unwrap();
    external.add_path(Path::new("other")).unwrap();
    external.write().unwrap();
    for client in [&local, &remote] {
        let large = vec![42_u8; 3 * 1024 * 1024 + 17];
        std::fs::write(root.join("large.bin"), &large).unwrap();
        std::fs::write(root.join("tracked"), "to stage\n").unwrap();
        std::fs::write(root.join("literal[1]"), "literal\n").unwrap();
        std::fs::write(root.join("literal1"), "must stay untracked\n").unwrap();
        let request = change(
            client,
            id,
            GitIndexChange::Stage,
            &["tracked", "literal[1]", "large.bin"],
        )
        .await;
        let result = client.execute(request.clone()).await.unwrap();
        assert!(matches!(result, Output::GitIndex(_)));
        assert_eq!(staged(&repository, "tracked"), "to stage\n");
        assert_eq!(staged(&repository, "other"), "external staged\n");
        let entry = disk_index(&repository)
            .get_path(Path::new("large.bin"), 0)
            .unwrap();
        assert_eq!(repository.find_blob(entry.id).unwrap().content(), large);
        assert!(
            disk_index(&repository)
                .get_path(Path::new("literal1"), 0)
                .is_none()
        );
        std::fs::write(root.join("tracked"), "after command\n").unwrap();
        assert_eq!(client.execute(request.clone()).await.unwrap(), result);
        assert_eq!(staged(&repository, "tracked"), "to stage\n");
        let mut stale = request.clone();
        stale.id = RequestId::new();
        assert_eq!(
            client.execute(stale).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        let unstage = change(
            client,
            id,
            GitIndexChange::Unstage,
            &["tracked", "literal[1]", "large.bin"],
        )
        .await;
        client.execute(unstage).await.unwrap();
        assert_eq!(staged(&repository, "tracked"), "base\n");
        assert_eq!(staged(&repository, "other"), "external staged\n");
        assert!(
            disk_index(&repository)
                .get_path(Path::new("large.bin"), 0)
                .is_none()
        );
        assert!(
            disk_index(&repository)
                .get_path(Path::new("literal[1]"), 0)
                .is_none()
        );
        assert_eq!(
            std::fs::read_to_string(root.join("tracked")).unwrap(),
            "after command\n"
        );
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn unborn_deletion_and_external_locks() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let repository = Repository::init(&root).unwrap();
    std::fs::write(root.join("new"), "new\n").unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let client = Client::new(node.local());
    let id = setup(&client, &root).await;
    let request = change(&client, id, GitIndexChange::Stage, &["new"]).await;
    std::fs::write(root.join(".git/index.lock"), "external lock").unwrap();
    assert_eq!(
        client.execute(request).await.unwrap_err().code,
        ErrorCode::Busy
    );
    assert_eq!(
        std::fs::read_to_string(root.join(".git/index.lock")).unwrap(),
        "external lock"
    );
    std::fs::remove_file(root.join(".git/index.lock")).unwrap();
    let request = change(&client, id, GitIndexChange::Stage, &["new"]).await;
    let candidate = root.join(format!(".git/sailry-index-{}", request.id));
    std::fs::write(&candidate, "previous recovery data").unwrap();
    assert!(client.execute(request).await.is_err());
    assert_eq!(
        std::fs::read_to_string(&candidate).unwrap(),
        "previous recovery data"
    );
    let request = change(&client, id, GitIndexChange::Stage, &["new"]).await;
    client.execute(request).await.unwrap();
    assert_eq!(staged(&repository, "new"), "new\n");
    std::fs::remove_file(root.join("new")).unwrap();
    let request = change(&client, id, GitIndexChange::Stage, &["new"]).await;
    client.execute(request).await.unwrap();
    assert!(disk_index(&repository).is_empty());
    assert!(!root.join(".git/index.lock").exists());
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn preserves_uncertain_result() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let repository = fixture(&root);
    let profile = temp.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    let client = Client::new(node.local());
    let id = setup(&client, &root).await;
    std::fs::write(root.join("tracked"), "published\n").unwrap();
    let request = change(&client, id, GitIndexChange::Stage, &["tracked"]).await;
    let database = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
    database.execute_batch("CREATE TRIGGER lose_git_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
    assert_eq!(
        client.execute(request.clone()).await.unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    assert_eq!(staged(&repository, "tracked"), "published\n");
    database
        .execute_batch("DROP TRIGGER lose_git_result")
        .unwrap();
    drop(database);
    node.shutdown().await.unwrap();
    std::fs::write(root.join("tracked"), "external\n").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("tracked")).unwrap();
    index.write().unwrap();
    let node = Node::start(&profile).await.unwrap();
    let client = Client::new(node.local());
    assert_eq!(
        client.execute(request).await.unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    assert_eq!(staged(&repository, "tracked"), "external\n");
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn rejected_updates_preserve_index() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let repository = fixture(&root);
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let client = Client::new(node.local());
    let id = setup(&client, &root).await;
    std::fs::write(root.join("tracked"), "first\n").unwrap();
    std::fs::write(root.join("other"), "second\n").unwrap();
    let first = change(&client, id, GitIndexChange::Stage, &["tracked"]).await;
    let second = change(&client, id, GitIndexChange::Stage, &["other"]).await;
    let (first, second) = tokio::join!(client.execute(first), client.execute(second));
    assert_ne!(first.is_ok(), second.is_ok());
    assert_eq!(
        first.err().or(second.err()).unwrap().code,
        ErrorCode::RevisionConflict
    );
    let before = std::fs::read(root.join(".git/index")).unwrap();
    std::fs::write(root.join(".gitignore"), "rejected\n").unwrap();
    std::fs::write(root.join("rejected"), "ignored\n").unwrap();
    std::fs::write(root.join("tracked"), "unpublished candidate\n").unwrap();
    let request = change(&client, id, GitIndexChange::Stage, &["tracked", "rejected"]).await;
    let error = client.execute(request).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(error.message.starts_with("rejected:"));
    assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), before);
    assert!(
        disk_index(&repository)
            .get_path(Path::new("rejected"), 0)
            .is_none()
    );
    assert!(!root.join(".git/index.lock").exists());
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn preserves_entry_modes() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let repository = fixture(&root);
    let outside = temp.path().join("outside");
    std::fs::write(&outside, "external content must not be staged").unwrap();
    symlink(&outside, root.join("link")).unwrap();
    std::fs::write(root.join("run"), "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(root.join("run"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(root.join(".gitignore"), "ignored\n").unwrap();
    std::fs::write(root.join("ignored"), "not selected").unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let client = Client::new(node.local());
    let id = setup(&client, &root).await;
    let request = change(&client, id, GitIndexChange::Stage, &["run", "link"]).await;
    client.execute(request).await.unwrap();
    let index = disk_index(&repository);
    assert_eq!(index.get_path(Path::new("run"), 0).unwrap().mode, 0o100755);
    assert_eq!(index.get_path(Path::new("link"), 0).unwrap().mode, 0o120000);
    assert_eq!(staged(&repository, "link"), outside.to_str().unwrap());
    let before = std::fs::read(root.join(".git/index")).unwrap();
    let request = change(&client, id, GitIndexChange::Stage, &["ignored"]).await;
    assert!(client.execute(request).await.is_err());
    assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), before);
    node.shutdown().await.unwrap();
}
