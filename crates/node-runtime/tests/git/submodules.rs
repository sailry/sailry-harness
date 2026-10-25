use super::*;

const MODULE: &str = "deps/module[1]";

struct Fixture {
    _directory: tempfile::TempDir,
    repository: Repository,
    child: Repository,
    node: Node,
    controller: Link,
    worktree: WorktreeId,
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("repository");
        let repository = Repository::init(&root).unwrap();
        repository.set_head("refs/heads/main").unwrap();
        let mut module = repository
            .submodule(
                "https://example.invalid/module.git",
                Path::new(MODULE),
                true,
            )
            .unwrap();
        let child = module.open().unwrap();
        child.set_head("refs/heads/main").unwrap();
        advance(&child, "base\n");
        module.add_finalize().unwrap();
        drop(module);
        record(&repository);
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let local = Client::new(node.local());
        let worktree = worktree(&local, &root).await;
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
            _directory: directory,
            repository,
            child,
            node,
            controller,
            worktree,
        }
    }

    fn clients(&self) -> [Client; 2] {
        [
            Client::new(self.node.local()),
            Client::new(self.controller.handle().remote(self.node.link().address())),
        ]
    }

    fn stage(&self) {
        self.repository
            .find_submodule(MODULE)
            .unwrap()
            .add_to_index(true)
            .unwrap();
    }

    async fn close(self) {
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}

fn record(repository: &Repository) -> git2::Oid {
    let mut index = repository.index().unwrap();
    let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
    let parent = match repository.head() {
        Ok(head) => Some(head.peel_to_commit().unwrap()),
        Err(error) if error.code() == git2::ErrorCode::UnbornBranch => None,
        Err(error) => panic!("fixture HEAD could not be read: {error}"),
    };
    let author = Signature::now("Fixture", "fixture@example.invalid").unwrap();
    repository
        .commit(
            Some("HEAD"),
            &author,
            &author,
            "Fixture",
            &tree,
            &parent.iter().collect::<Vec<_>>(),
        )
        .unwrap()
}

fn advance(repository: &Repository, content: &str) -> git2::Oid {
    std::fs::write(repository.workdir().unwrap().join("file.txt"), content).unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("file.txt")).unwrap();
    index.write().unwrap();
    record(repository)
}

fn changed(patch: &GitDiff, old: &str, new: &str) {
    assert_eq!(patch.path, MODULE);
    assert!(
        patch.text.contains(&format!("-Subproject commit {old}\n")),
        "{patch:?}"
    );
    assert!(
        patch.text.contains(&format!("+Subproject commit {new}\n")),
        "{patch:?}"
    );
    assert_eq!((patch.additions, patch.deletions), (1, 1));
    assert!(!patch.binary);
    assert!(!patch.truncated);
}

#[tokio::test]
async fn shares_clean_dirty_and_staged_diffs() {
    let fixture = Fixture::new().await;
    let base = fixture.child.head().unwrap().target().unwrap();
    for client in fixture.clients() {
        for scope in [
            GitDiffScope::All,
            GitDiffScope::Staged,
            GitDiffScope::Unstaged,
        ] {
            let patch = diff(&client, fixture.worktree, MODULE, scope).await;
            assert_eq!(patch.path, MODULE);
            assert_eq!(patch.scope, scope);
            assert!(patch.text.is_empty(), "{patch:?}");
            assert_eq!((patch.additions, patch.deletions), (0, 0));
            assert!(!patch.binary);
            assert!(!patch.truncated);
        }
    }
    let file = fixture.child.workdir().unwrap().join("file.txt");
    std::fs::write(&file, "dirty source sentinel\n").unwrap();
    let index = std::fs::read(fixture.repository.path().join("index")).unwrap();
    let child_index = std::fs::read(fixture.child.path().join("index")).unwrap();
    for client in fixture.clients() {
        for scope in [GitDiffScope::All, GitDiffScope::Unstaged] {
            let patch = diff(&client, fixture.worktree, MODULE, scope).await;
            changed(&patch, &base.to_string(), &format!("{base}-dirty"));
            assert!(!patch.text.contains("dirty source sentinel"));
        }
        assert!(
            diff(&client, fixture.worktree, MODULE, GitDiffScope::Staged)
                .await
                .text
                .is_empty()
        );
    }
    assert_eq!(
        std::fs::read(fixture.repository.path().join("index")).unwrap(),
        index
    );
    assert_eq!(
        std::fs::read(fixture.child.path().join("index")).unwrap(),
        child_index
    );
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "dirty source sentinel\n"
    );

    let staged = advance(&fixture.child, "staged\n");
    fixture.stage();
    let working = advance(&fixture.child, "working\n");
    let index = std::fs::read(fixture.repository.path().join("index")).unwrap();
    for client in fixture.clients() {
        for (scope, old, new) in [
            (GitDiffScope::All, base, working),
            (GitDiffScope::Staged, base, staged),
            (GitDiffScope::Unstaged, staged, working),
        ] {
            let patch = diff(&client, fixture.worktree, MODULE, scope).await;
            assert_eq!(patch.scope, scope);
            changed(&patch, &old.to_string(), &new.to_string());
        }
    }
    assert_eq!(
        std::fs::read(fixture.repository.path().join("index")).unwrap(),
        index
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "working\n");

    let mut index = fixture.repository.index().unwrap();
    index.remove_path(Path::new(MODULE)).unwrap();
    index.write().unwrap();
    for client in fixture.clients() {
        let patch = diff(&client, fixture.worktree, MODULE, GitDiffScope::Staged).await;
        assert!(patch.text.contains("deleted file mode 160000"));
        assert!(patch.text.contains(&format!("-Subproject commit {base}\n")));
        assert_eq!((patch.additions, patch.deletions), (0, 1));
        assert!(!patch.binary);
        assert!(!patch.truncated);
    }
    fixture.close().await;
}

#[tokio::test]
async fn retains_pinned_commit_changes() {
    let fixture = Fixture::new().await;
    let base = fixture.child.head().unwrap().target().unwrap();
    let staged = advance(&fixture.child, "staged\n");
    fixture.stage();
    let pinned = record(&fixture.repository);
    let working = advance(&fixture.child, "working\n");
    fixture.stage();
    let current = record(&fixture.repository);
    std::fs::write(fixture.child.workdir().unwrap().join("file.txt"), "dirty\n").unwrap();
    let index = std::fs::read(fixture.repository.path().join("index")).unwrap();
    for client in fixture.clients() {
        for command in [
            Command::ReadGitCommit {
                worktree: fixture.worktree,
                commit: pinned.to_string(),
            },
            Command::ReadGitShow {
                worktree: fixture.worktree,
                revision: pinned.to_string(),
                path: None,
                offset: 0,
                limit: 200,
            },
        ] {
            let Output::GitCommit(commit) = client.execute(client.prepare(command)).await.unwrap()
            else {
                panic!("commit patch expected")
            };
            assert_eq!(commit.entry.id, pinned.to_string());
            assert_eq!(commit.files.len(), 1);
            changed(&commit.files[0], &base.to_string(), &staged.to_string());
            assert!(!commit.files[0].text.contains(&working.to_string()));
            assert!(!commit.files[0].text.contains("-dirty"));
            assert_eq!((commit.additions, commit.deletions), (1, 1));
            assert!(!commit.binary);
            assert!(!commit.truncated);
        }
    }
    assert_eq!(fixture.repository.head().unwrap().target(), Some(current));
    assert_eq!(
        std::fs::read(fixture.repository.path().join("index")).unwrap(),
        index
    );
    fixture.close().await;
}

#[tokio::test]
async fn preserves_path_and_file_boundaries() {
    let fixture = Fixture::new().await;
    let root = fixture.repository.workdir().unwrap();
    std::fs::create_dir(root.join("ordinary")).unwrap();
    std::fs::write(root.join("ordinary/file.txt"), "not a Gitlink\n").unwrap();
    Repository::init(root.join("untracked-repository")).unwrap();
    for client in fixture.clients() {
        for path in [
            "ordinary",
            "untracked-repository",
            "deps",
            ".git",
            "deps/.git/config",
            "../outside",
        ] {
            let error = client
                .execute(client.prepare(Command::ReadGitDiff {
                    worktree: fixture.worktree,
                    path: path.into(),
                    scope: GitDiffScope::All,
                }))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidRequest, "{path}");
        }
        for command in [
            Command::ReadFile {
                worktree: fixture.worktree,
                path: MODULE.into(),
            },
            Command::ReadGitShow {
                worktree: fixture.worktree,
                revision: "HEAD".into(),
                path: Some(MODULE.into()),
                offset: 0,
                limit: 200,
            },
            Command::ReadGitBlame {
                worktree: fixture.worktree,
                revision: None,
                path: MODULE.into(),
                offset: 0,
                limit: 200,
            },
        ] {
            let error = client.execute(client.prepare(command)).await.unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidRequest);
        }
    }
    #[cfg(unix)]
    {
        let relocated = fixture._directory.path().join("relocated");
        std::fs::rename(root.join("deps"), &relocated).unwrap();
        std::os::unix::fs::symlink(&relocated, root.join("deps")).unwrap();
        for client in fixture.clients() {
            assert!(
                client
                    .execute(client.prepare(Command::ReadGitDiff {
                        worktree: fixture.worktree,
                        path: MODULE.into(),
                        scope: GitDiffScope::All,
                    }))
                    .await
                    .is_err()
            );
        }
    }
    fixture.close().await;
}
