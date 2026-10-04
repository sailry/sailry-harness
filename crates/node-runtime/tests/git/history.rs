use super::*;

async fn log(client: &Client, worktree: WorktreeId, limit: usize) -> GitLog {
    let Output::GitLog(log) = client
        .execute(client.prepare(Command::ReadGitLog {
            worktree,
            limit,
            cursor: None,
        }))
        .await
        .unwrap()
    else {
        panic!()
    };
    log
}

async fn detail(client: &Client, worktree: WorktreeId, commit: git2::Oid) -> GitCommit {
    let Output::GitCommit(commit) = client
        .execute(client.prepare(Command::ReadGitCommit {
            worktree,
            commit: commit.to_string(),
        }))
        .await
        .unwrap()
    else {
        panic!()
    };
    commit
}

#[tokio::test]
async fn shares_read_only_history() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    std::fs::write(root.join("中文.txt"), "原始内容\n").unwrap();
    std::fs::write(
        root.join("build notes.md"),
        "diff --git content, not a file boundary\n",
    )
    .unwrap();
    commit(&repository);
    let first = repository.head().unwrap().peel_to_commit().unwrap();
    std::fs::write(root.join("中文.txt"), "更新内容\n").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("中文.txt")).unwrap();
    std::fs::remove_file(root.join("build notes.md")).unwrap();
    index.remove_path(Path::new("build notes.md")).unwrap();
    index.write().unwrap();
    let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
    let signature = Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let message = format!("中文提交\n\n{}", "详细说明".repeat(1200));
    let second = repository
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            &message,
            &tree,
            &[&first],
        )
        .unwrap();
    repository
        .branch("feature", &repository.find_commit(second).unwrap(), false)
        .unwrap();
    repository
        .tag_lightweight("v1", first.as_object(), false)
        .unwrap();
    // A disconnected branch must not appear in the current HEAD traversal.
    let unrelated = repository
        .commit(
            Some("refs/heads/unrelated"),
            &signature,
            &signature,
            "Unrelated",
            &tree,
            &[],
        )
        .unwrap();
    std::fs::write(root.join("中文.txt"), "Working copy must stay unchanged\n").unwrap();
    let index_before = std::fs::read(repository.path().join("index")).unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let id = worktree(&local, &root).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let invitation = node.link().invite().unwrap();
    let address = controller.handle().pair(invitation.ticket()).await.unwrap();
    let transport = controller.handle().remote(address);
    let remote = Client::new(transport.clone());
    for client in [&local, &remote] {
        let page = log(client, id, 1).await;
        assert!(page.truncated);
        assert_eq!(page.entries[0].id, second.to_string());
        assert!(page.entries[0].truncated);
        assert!(page.entries[0].message.len() <= 4096);
        assert!(page.entries[0].message.starts_with("中文提交"));
        let history = log(client, id, MAX_GIT_COMMITS).await;
        assert_eq!(history.entries.len(), 2);
        assert!(!history.truncated);
        assert!(
            history.entries[0]
                .references
                .contains(&"heads/feature".into())
        );
        assert!(history.entries[1].references.contains(&"tags/v1".into()));
        assert!(
            history
                .entries
                .iter()
                .all(|entry| entry.id != unrelated.to_string())
        );
        let patch = detail(client, id, second).await;
        assert_eq!(patch.files.len(), 2);
        let text = patch
            .files
            .iter()
            .find(|file| file.path == "中文.txt")
            .unwrap();
        assert!(text.text.contains("-原始内容"));
        assert!(text.text.contains("+更新内容"));
        assert_eq!((text.additions, text.deletions), (1, 1));
        let deleted = patch
            .files
            .iter()
            .find(|file| file.path == "build notes.md")
            .unwrap();
        assert!(
            deleted
                .text
                .contains("-diff --git content, not a file boundary")
        );
        assert_eq!((deleted.additions, deleted.deletions), (0, 1));
        assert_eq!((patch.additions, patch.deletions), (1, 2));
        assert!(
            patch
                .files
                .iter()
                .all(|file| !file.text.contains("Working copy"))
        );
        let initial = detail(client, id, first.id()).await;
        assert_eq!(initial.files.len(), 2);
        assert!(
            initial
                .files
                .iter()
                .any(|file| file.path == "中文.txt" && file.text.contains("+原始内容"))
        );
        assert!(initial.files.iter().any(|file| {
            file.path == "build notes.md"
                && file
                    .text
                    .contains("+diff --git content, not a file boundary")
        }));
        for invalid in ["HEAD", "HEAD~1", "../file", "", "1234567"] {
            let fault = client
                .execute(client.prepare(Command::ReadGitCommit {
                    worktree: id,
                    commit: invalid.into(),
                }))
                .await
                .unwrap_err();
            assert_eq!(fault.code, ErrorCode::InvalidRequest);
        }
        let fault = client
            .execute(client.prepare(Command::ReadGitLog {
                worktree: WorktreeId::new(),
                limit: 10,
                cursor: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(fault.code, ErrorCode::NotFound);
        for limit in [0, MAX_GIT_COMMITS + 1] {
            assert_eq!(
                client
                    .execute(client.prepare(Command::ReadGitLog {
                        worktree: id,
                        limit,
                        cursor: None,
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
    }
    let admission = transport
        .dispatch(remote.prepare(Command::ReadGitLog {
            worktree: id,
            limit: 10,
            cursor: None,
        }))
        .await
        .unwrap();
    assert!(!admission.receipt.durable);
    admission.completion.await.unwrap().unwrap();
    assert_eq!(
        std::fs::read(repository.path().join("index")).unwrap(),
        index_before
    );
    assert_eq!(repository.head().unwrap().target(), Some(second));
    assert_eq!(
        std::fs::read_to_string(root.join("中文.txt")).unwrap(),
        "Working copy must stay unchanged\n"
    );
    let database = rusqlite::Connection::open(node.profile().join("storage/node.sqlite3")).unwrap();
    assert_eq!(
        database
            .query_row("SELECT count(*) FROM requests", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn bounds_repository_history() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = Repository::init(&root).unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let client = Client::new(node.local());
    let id = worktree(&client, &root).await;
    assert_eq!(log(&client, id, 10).await.kind, RepositoryKind::Unborn);
    let directory = temp.path().join("directory");
    std::fs::create_dir(&directory).unwrap();
    let ordinary = worktree(&client, &directory).await;
    assert_eq!(
        log(&client, ordinary, 10).await.kind,
        RepositoryKind::Directory
    );
    std::fs::write(
        root.join("large.txt"),
        "long line for bounded history output\n".repeat(2000),
    )
    .unwrap();
    std::fs::write(root.join("binary.dat"), b"\0\x01\x02").unwrap();
    commit(&repository);
    let head = repository.head().unwrap().target().unwrap();
    repository.set_head_detached(head).unwrap();
    assert_eq!(log(&client, id, 10).await.head, Some(head.to_string()));
    let patch = detail(&client, id, head).await;
    assert!(patch.truncated);
    assert!(
        patch
            .files
            .iter()
            .map(|file| file.text.len())
            .sum::<usize>()
            <= MAX_DIFF_BYTES
    );
    assert!(
        patch
            .files
            .iter()
            .any(|file| file.path == "binary.dat" && file.binary)
    );
    assert!(
        patch
            .files
            .iter()
            .any(|file| file.path == "large.txt" && file.truncated)
    );
    assert!(patch.binary);
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn retains_pagination_root() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = Repository::init(&root).unwrap();
    std::fs::write(root.join("base.txt"), "base\n").unwrap();
    commit(&repository);
    let base = repository.head().unwrap().peel_to_commit().unwrap();
    let tree = base.tree().unwrap();
    let signature = Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let mut main = base.id();
    let mut branch = base.id();
    for position in 0..12 {
        for (side, parent) in [&mut main, &mut branch].into_iter().enumerate() {
            *parent = repository
                .commit(
                    None,
                    &signature,
                    &signature,
                    &format!("Commit {position} side {side}"),
                    &tree,
                    &[&repository.find_commit(*parent).unwrap()],
                )
                .unwrap();
        }
    }
    repository.set_head_detached(main).unwrap();
    let merged = repository
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            "Merge",
            &tree,
            &[
                &repository.find_commit(main).unwrap(),
                &repository.find_commit(branch).unwrap(),
            ],
        )
        .unwrap();
    let mut walk = repository.revwalk().unwrap();
    walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)
        .unwrap();
    walk.push(merged).unwrap();
    let expected = walk.map(|oid| oid.unwrap().to_string()).collect::<Vec<_>>();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let id = worktree(&local, &root).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let invitation = node.link().invite().unwrap();
    let address = controller.handle().pair(invitation.ticket()).await.unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for client in [&local, &remote] {
        repository.set_head_detached(merged).unwrap();
        let mut page = log(client, id, 7).await;
        let first = page.clone();
        let parent = repository.find_commit(merged).unwrap();
        repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "Arrived while paging",
                &tree,
                &[&parent],
            )
            .unwrap();
        let mut all = page
            .entries
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>();
        while let Some(cursor) = page.next {
            let Output::GitLog(next) = client
                .execute(client.prepare(Command::ReadGitLog {
                    worktree: id,
                    limit: 7,
                    cursor: Some(cursor),
                }))
                .await
                .unwrap()
            else {
                panic!()
            };
            assert_eq!(next.head, Some(merged.to_string()));
            assert_eq!(next.offset, all.len());
            all.extend(next.entries.iter().map(|entry| entry.id.clone()));
            page = next;
        }
        assert_eq!(all, expected);
        assert!(!page.truncated);
        let Output::GitLog(previous) = client
            .execute(client.prepare(Command::ReadGitLog {
                worktree: id,
                limit: 7,
                cursor: Some(GitLogCursor {
                    head: merged.to_string(),
                    offset: 0,
                }),
            }))
            .await
            .unwrap()
        else {
            panic!()
        };
        // Reference labels may move, but the ordered commit page is immutable.
        assert_eq!(
            previous.entries.iter().map(|e| &e.id).collect::<Vec<_>>(),
            first.entries.iter().map(|e| &e.id).collect::<Vec<_>>()
        );
        for cursor in [
            GitLogCursor {
                head: "HEAD".into(),
                offset: 7,
            },
            GitLogCursor {
                head: merged.to_string(),
                offset: usize::MAX,
            },
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(Command::ReadGitLog {
                        worktree: id,
                        limit: 7,
                        cursor: Some(cursor)
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        assert_ne!(log(client, id, 7).await.head, first.head);
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
