#[path = "git_commit/actions.rs"]
mod actions;
#[path = "git_commit/ignore.rs"]
mod ignore;
#[path = "git_commit/management.rs"]
mod management;
#[path = "git_commit/pruning.rs"]
mod pruning;

use git2::{Oid, Repository, RepositoryInitOptions};
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::*;
use std::path::Path;

struct Fixture {
    temp: tempfile::TempDir,
    node: Node,
    client: Client,
    repository: Repository,
    worktree: WorktreeId,
}

impl Fixture {
    async fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        let repository =
            Repository::init_opts(&root, RepositoryInitOptions::new().initial_head("main"))
                .unwrap();
        let mut config = repository.config().unwrap();
        config.set_str("user.name", "Execution Node").unwrap();
        config
            .set_str("user.email", "node@example.invalid")
            .unwrap();
        config.set_bool("commit.gpgsign", false).unwrap();
        let node = Node::start(temp.path().join("node")).await.unwrap();
        let client = Client::new(node.local());
        client
            .execute(client.prepare(Command::RegisterProject {
                name: "Commit fixture".into(),
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
        Self {
            temp,
            node,
            client,
            repository,
            worktree: snapshot.worktrees[0].id,
        }
    }

    fn stage(&self, text: &str) {
        std::fs::write(self.repository.workdir().unwrap().join("README.md"), text).unwrap();
        let mut index = self.repository.index().unwrap();
        index.read(true).unwrap();
        index.add_path(Path::new("README.md")).unwrap();
        index.write().unwrap();
    }

    async fn request(&self, client: &Client, message: &str, amend: bool) -> Request {
        let Output::GitStatus(status) = client
            .execute(client.prepare(Command::InspectGit {
                worktree: self.worktree,
            }))
            .await
            .unwrap()
        else {
            panic!()
        };
        client.prepare(Command::CreateGitCommit {
            worktree: self.worktree,
            message: message.into(),
            amend,
            options: Default::default(),
            expected_index: status.index_revision.unwrap(),
            expected_head: status.head,
            expected_branch: status.branch,
        })
    }

    async fn commit(&self, message: &str, amend: bool) -> Oid {
        let output = self
            .client
            .execute(self.request(&self.client, message, amend).await)
            .await
            .unwrap();
        let Output::GitCommitCreated { id, .. } = output else {
            panic!()
        };
        Oid::from_str(&id).unwrap()
    }
}

#[tokio::test]
async fn local_and_remote() {
    let fixture = Fixture::new().await;
    let controller = Link::controller(
        fixture.temp.path().join("controller"),
        NetworkScope::default(),
    )
    .await
    .unwrap();
    let invite = fixture.node.link().invite().unwrap();
    let address = controller.handle().pair(invite.ticket()).await.unwrap();
    let remote = Client::new(controller.handle().remote(address));
    let mut previous = None;
    for (position, client) in [&fixture.client, &remote].into_iter().enumerate() {
        let text = format!("staged {position}\n");
        fixture.stage(&text);
        let index: Vec<_> = fixture
            .repository
            .index()
            .unwrap()
            .iter()
            .map(|entry| (entry.path, entry.id, entry.mode))
            .collect();
        let request = fixture
            .request(client, "Commit staged content\n\nDetailed message", false)
            .await;
        std::fs::write(
            fixture.repository.workdir().unwrap().join("README.md"),
            "unstaged stays\n",
        )
        .unwrap();
        let result = client.execute(request.clone()).await.unwrap();
        let Output::GitCommitCreated { ref id, .. } = result else {
            panic!()
        };
        let id = Oid::from_str(id).unwrap();
        let commit = fixture.repository.find_commit(id).unwrap();
        assert_eq!(commit.author().name().unwrap(), "Execution Node");
        assert_eq!(
            commit.message().unwrap(),
            "Commit staged content\n\nDetailed message\n"
        );
        assert_eq!(commit.parent_count(), position);
        if let Some(parent) = previous {
            assert_eq!(commit.parent_id(0).unwrap(), parent);
        }
        let tree = commit.tree().unwrap();
        let blob = fixture
            .repository
            .find_blob(tree.get_name("README.md").unwrap().id())
            .unwrap();
        assert_eq!(blob.content(), text.as_bytes());
        assert_eq!(fixture.repository.head().unwrap().target(), Some(id));
        assert_eq!(
            fixture
                .repository
                .reflog("HEAD")
                .unwrap()
                .get(0)
                .unwrap()
                .id_new(),
            id
        );
        assert_eq!(
            fixture
                .repository
                .reflog("refs/heads/main")
                .unwrap()
                .get(0)
                .unwrap()
                .id_new(),
            id
        );
        assert_eq!(
            // Native Git may add the cache-tree extension while preserving staged entries.
            git2::Index::open(&fixture.repository.path().join("index"))
                .unwrap()
                .iter()
                .map(|entry| (entry.path, entry.id, entry.mode))
                .collect::<Vec<_>>(),
            index
        );
        assert_eq!(
            std::fs::read_to_string(fixture.repository.workdir().unwrap().join("README.md"))
                .unwrap(),
            "unstaged stays\n"
        );
        assert_eq!(client.execute(request).await.unwrap(), result);
        assert!(!fixture.repository.path().join("index.lock").exists());
        assert!(!fixture.repository.path().join("HEAD.lock").exists());
        previous = Some(id);
    }
    controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn amend_detached() {
    let fixture = Fixture::new().await;
    fixture.stage("initial\n");
    let initial = fixture.commit("Initial", false).await;
    fixture.repository.set_head_detached(initial).unwrap();
    fixture.stage("second\n");
    let second = fixture.commit("Detached", false).await;
    let original = fixture.repository.find_commit(second).unwrap();
    fixture
        .repository
        .config()
        .unwrap()
        .set_str("user.name", "New Committer")
        .unwrap();
    let amended = fixture.commit("Amended message", true).await;
    let commit = fixture.repository.find_commit(amended).unwrap();
    assert_eq!(commit.parent_id(0).unwrap(), initial);
    assert_eq!(commit.tree_id(), original.tree_id());
    assert!(commit.author() == original.author());
    assert_eq!(commit.committer().name().unwrap(), "New Committer");
    assert!(fixture.repository.head_detached().unwrap());
    assert_eq!(
        fixture.repository.refname_to_id("refs/heads/main").unwrap(),
        initial
    );
    fixture.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn stale_revisions() {
    let fixture = Fixture::new().await;
    fixture.stage("initial\n");
    let initial = fixture.commit("Initial", false).await;
    fixture.stage("changed\n");
    let request = fixture
        .request(&fixture.client, "Stale branch", false)
        .await;
    fixture
        .repository
        .branch(
            "other",
            &fixture.repository.find_commit(initial).unwrap(),
            false,
        )
        .unwrap();
    fixture.repository.set_head("refs/heads/other").unwrap();
    assert_eq!(
        fixture.client.execute(request).await.unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    let request = fixture.request(&fixture.client, "Stale index", false).await;
    fixture.stage("later staged\n");
    assert_eq!(
        fixture.client.execute(request).await.unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    let request = fixture.request(&fixture.client, "Stale HEAD", false).await;
    let second = fixture.commit("Other command", false).await;
    assert_eq!(
        fixture.client.execute(request).await.unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(fixture.repository.head().unwrap().target(), Some(second));
    assert_eq!(
        fixture.repository.refname_to_id("refs/heads/main").unwrap(),
        initial
    );
    fixture.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn locks_and_competing_commands() {
    let fixture = Fixture::new().await;
    fixture.stage("initial\n");
    for name in ["index.lock", "HEAD.lock", "refs/heads/main.lock"] {
        let lock = fixture.repository.path().join(name);
        std::fs::write(&lock, "external lock").unwrap();
        let request = fixture.request(&fixture.client, "Locked", false).await;
        assert_eq!(
            fixture.client.execute(request).await.unwrap_err().code,
            ErrorCode::Busy
        );
        assert_eq!(std::fs::read_to_string(&lock).unwrap(), "external lock");
        std::fs::remove_file(lock).unwrap();
    }
    let first = fixture.request(&fixture.client, "First", false).await;
    let second = fixture.request(&fixture.client, "Second", false).await;
    let (first, second) = tokio::join!(
        fixture.client.execute(first),
        fixture.client.execute(second)
    );
    assert_ne!(first.is_ok(), second.is_ok());
    assert_eq!(
        first.err().or(second.err()).unwrap().code,
        ErrorCode::RevisionConflict
    );
    fixture.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn invalid_inputs() {
    let fixture = Fixture::new().await;
    let request = fixture.request(&fixture.client, "No changes", false).await;
    assert_eq!(
        fixture.client.execute(request).await.unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    fixture.stage("initial\n");
    for message in ["  \n", "invalid\0message"] {
        let request = fixture.request(&fixture.client, message, false).await;
        assert_eq!(
            fixture.client.execute(request).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
    fixture
        .repository
        .config()
        .unwrap()
        .set_str("user.name", "")
        .unwrap();
    let request = fixture.request(&fixture.client, "No identity", false).await;
    assert_eq!(
        fixture.client.execute(request).await.unwrap_err().code,
        ErrorCode::NotConfigured
    );
    fixture
        .repository
        .config()
        .unwrap()
        .set_str("user.name", "Fixture")
        .unwrap();
    fixture
        .repository
        .config()
        .unwrap()
        .set_bool("commit.gpgsign", true)
        .unwrap();
    fixture
        .repository
        .config()
        .unwrap()
        .set_str("gpg.program", "sailry-missing-signing-fixture")
        .unwrap();
    let request = fixture
        .request(&fixture.client, "Requires signature", false)
        .await;
    assert_eq!(
        fixture.client.execute(request).await.unwrap_err().code,
        ErrorCode::Unavailable
    );
    assert!(fixture.repository.head().is_err());
    assert!(!fixture.repository.path().join("index.lock").exists());
    fixture.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn restart_after_result_loss() {
    let fixture = Fixture::new().await;
    fixture.stage("initial\n");
    let request = fixture
        .request(&fixture.client, "Published once", false)
        .await;
    let profile = fixture.temp.path().join("node");
    let database = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
    database.execute_batch("CREATE TRIGGER lose_commit_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
    assert_eq!(
        fixture
            .client
            .execute(request.clone())
            .await
            .unwrap_err()
            .code,
        ErrorCode::OutcomeUnknown
    );
    let head = fixture.repository.head().unwrap().target().unwrap();
    database
        .execute_batch("DROP TRIGGER lose_commit_result")
        .unwrap();
    drop(database);
    fixture.node.shutdown().await.unwrap();
    let node = Node::start(&profile).await.unwrap();
    let client = Client::new(node.local());
    assert_eq!(
        client.execute(request).await.unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    assert_eq!(fixture.repository.head().unwrap().target(), Some(head));
    assert_eq!(
        fixture.repository.find_commit(head).unwrap().parent_count(),
        0
    );
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn merge_parents() {
    let fixture = Fixture::new().await;
    fixture.stage("base\n");
    let base = fixture.commit("Base", false).await;
    let parent = fixture.repository.find_commit(base).unwrap();
    let signature = fixture.repository.signature().unwrap();
    let other = fixture
        .repository
        .commit(
            Some("refs/heads/other"),
            &signature,
            &signature,
            "Other",
            &parent.tree().unwrap(),
            &[&parent],
        )
        .unwrap();
    std::fs::write(
        fixture.repository.path().join("MERGE_HEAD"),
        format!("{other}\n"),
    )
    .unwrap();
    std::fs::write(fixture.repository.path().join("MERGE_MODE"), "no-ff").unwrap();
    std::fs::write(fixture.repository.path().join("MERGE_MSG"), "Merge other\n").unwrap();
    let id = fixture.commit("Merge other", false).await;
    let merged = fixture.repository.find_commit(id).unwrap();
    assert_eq!(merged.parent_ids().collect::<Vec<_>>(), [base, other]);
    assert_eq!(fixture.repository.state(), git2::RepositoryState::Clean);
    assert!(!fixture.repository.path().join("MERGE_HEAD").exists());
    fixture.node.shutdown().await.unwrap();
}
