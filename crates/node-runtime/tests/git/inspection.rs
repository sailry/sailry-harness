use super::*;

async fn show(
    client: &Client,
    worktree: WorktreeId,
    revision: &str,
    path: &str,
    offset: usize,
    limit: usize,
) -> GitFile {
    let Output::GitFile(file) = client
        .execute(client.prepare(Command::ReadGitShow {
            worktree,
            revision: revision.into(),
            path: Some(path.into()),
            offset,
            limit,
        }))
        .await
        .unwrap()
    else {
        panic!("committed file expected")
    };
    file
}

async fn blame(
    client: &Client,
    worktree: WorktreeId,
    revision: Option<&str>,
    path: &str,
    offset: usize,
    limit: usize,
) -> GitBlame {
    let Output::GitBlame(lines) = client
        .execute(client.prepare(Command::ReadGitBlame {
            worktree,
            revision: revision.map(str::to_owned),
            path: path.into(),
            offset,
            limit,
        }))
        .await
        .unwrap()
    else {
        panic!("line authorship expected")
    };
    lines
}

#[tokio::test]
async fn shares_committed_file_pages_and_authorship() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = Repository::init(&root).unwrap();
    let path = "-quoted [资料].txt";
    std::fs::write(root.join(path), "first\nsecond\nthird\n").unwrap();
    #[cfg(unix)]
    {
        std::fs::write(temp.path().join("private"), "external secret sentinel").unwrap();
        std::os::unix::fs::symlink(temp.path().join("private"), root.join("link")).unwrap();
    }
    commit(&repository);
    let first = repository.head().unwrap().peel_to_commit().unwrap();
    std::fs::write(root.join(path), "first\nchanged\nthird\n").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new(path)).unwrap();
    index.write().unwrap();
    let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
    let author = Signature::now("Second", "second@example.invalid").unwrap();
    let second = repository
        .commit(
            Some("HEAD"),
            &author,
            &author,
            "Change one line",
            &tree,
            &[&first],
        )
        .unwrap();
    std::fs::write(root.join(path), "uncommitted must stay unchanged\n").unwrap();
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
        repository.set_head_detached(second).unwrap();
        let page = show(client, id, "HEAD", path, 0, 2).await;
        assert_eq!(page.commit, second.to_string());
        assert_eq!(page.text, "1: first\n2: changed\n");
        assert_eq!(page.total_lines, 3);
        assert_eq!(page.next, Some(2));
        assert!(page.truncated);
        repository.set_head_detached(first.id()).unwrap();
        let next = show(client, id, &page.commit, path, page.next.unwrap(), 2).await;
        assert_eq!(next.text, "3: third\n");
        assert_eq!(next.next, None);
        assert!(!next.truncated);
        assert_eq!(
            show(client, id, "HEAD", path, 1, 1).await.text,
            "2: second\n"
        );
        repository.set_head_detached(second).unwrap();
        let authors = blame(client, id, None, path, 0, 2).await;
        assert_eq!(authors.commit, second.to_string());
        assert_eq!(authors.next, Some(2));
        assert_eq!(authors.entries[0].line, 1);
        assert_eq!(authors.entries[0].commit, first.id().to_string());
        assert_eq!(authors.entries[0].author, "Fixture");
        assert_eq!(authors.entries[1].commit, second.to_string());
        assert_eq!(authors.entries[1].author, "Second");
        assert_eq!(authors.entries[1].text, "changed");
        let next = blame(client, id, Some(&authors.commit), path, 2, 2).await;
        assert_eq!(next.entries.len(), 1);
        assert_eq!(next.entries[0].original_line, 3);
        assert_eq!(next.entries[0].commit, first.id().to_string());
        assert!(!next.truncated);
        let Output::GitCommit(patch) = client
            .execute(client.prepare(Command::ReadGitShow {
                worktree: id,
                revision: "HEAD~1".into(),
                path: None,
                offset: 0,
                limit: 200,
            }))
            .await
            .unwrap()
        else {
            panic!("commit patch expected")
        };
        assert_eq!(patch.entry.id, first.id().to_string());
        assert!(patch.files[0].text.contains("+second\n"));
        for path in [
            "../outside",
            ".git/config",
            "/etc/passwd",
            "a\\b",
            "a/../b",
            "",
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(Command::ReadGitBlame {
                        worktree: id,
                        path: path.into(),
                        revision: None,
                        offset: 0,
                        limit: 2,
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        for revision in ["--output=outside", "HEAD:path", "HEAD\n", ""] {
            assert_eq!(
                client
                    .execute(client.prepare(Command::ReadGitShow {
                        worktree: id,
                        revision: revision.into(),
                        path: Some(path.into()),
                        offset: 0,
                        limit: 2,
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        #[cfg(unix)]
        {
            assert_eq!(
                client
                    .execute(client.prepare(Command::ReadGitShow {
                        worktree: id,
                        revision: "HEAD".into(),
                        path: Some("link".into()),
                        offset: 0,
                        limit: 2,
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
            assert_eq!(
                std::fs::read_to_string(temp.path().join("private")).unwrap(),
                "external secret sentinel"
            );
        }
        for (offset, limit) in [(4, 2), (0, 0), (0, MAX_GIT_LINES + 1), (usize::MAX, 1)] {
            assert_eq!(
                client
                    .execute(client.prepare(Command::ReadGitBlame {
                        worktree: id,
                        revision: None,
                        path: path.into(),
                        offset,
                        limit,
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
    }
    let admission = transport
        .dispatch(remote.prepare(Command::ReadGitBlame {
            worktree: id,
            revision: None,
            path: path.into(),
            offset: 0,
            limit: 2,
        }))
        .await
        .unwrap();
    assert!(!admission.receipt.durable);
    admission.completion.await.unwrap().unwrap();
    assert_eq!(
        std::fs::read(repository.path().join("index")).unwrap(),
        index_before
    );
    assert_eq!(
        std::fs::read_to_string(root.join(path)).unwrap(),
        "uncommitted must stay unchanged\n"
    );
    let database = rusqlite::Connection::open(node.profile().join("storage/node.sqlite3")).unwrap();
    assert_eq!(
        database
            .query_row("SELECT count(*) FROM requests", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(database);
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn reports_bounded_text_and_binary_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = Repository::init(&root).unwrap();
    std::fs::write(
        root.join("large.txt"),
        format!("{}\nlast\n", "🙂".repeat(MAX_DIFF_BYTES)),
    )
    .unwrap();
    std::fs::write(root.join("binary.dat"), b"\0\x01").unwrap();
    commit(&repository);
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
        let page = show(client, id, "HEAD", "large.txt", 0, 200).await;
        assert!(page.truncated);
        assert_eq!(page.truncated_line, Some(1));
        assert!(page.text.len() <= MAX_DIFF_BYTES);
        assert_eq!(page.next, Some(1));
        assert_eq!(
            show(client, id, &page.commit, "large.txt", 1, 200)
                .await
                .text,
            "2: last\n"
        );
        let authors = blame(client, id, None, "large.txt", 0, 200).await;
        assert!(authors.entries[0].truncated);
        assert_eq!(authors.next, Some(1));
        assert!(authors.entries[0].text.len() <= MAX_DIFF_BYTES);
        let binary = show(client, id, "HEAD", "binary.dat", 0, 200).await;
        assert!(binary.binary);
        assert!(binary.text.is_empty());
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadGitBlame {
                    worktree: id,
                    revision: None,
                    path: "binary.dat".into(),
                    offset: 0,
                    limit: 200,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
