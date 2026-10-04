use super::*;

#[tokio::test]
async fn local_and_remote_resolution() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    std::fs::write(root.join("file"), "base").unwrap();
    commit(&repository);
    let base = repository.head().unwrap().peel_to_commit().unwrap();
    let author = Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let tip = repository
        .commit(
            Some("HEAD"),
            &author,
            &author,
            "Next",
            &base.tree().unwrap(),
            &[&base],
        )
        .unwrap();
    repository.branch("feature/中文", &base, false).unwrap();
    repository
        .tag("release", base.as_object(), &author, "Fixture tag", false)
        .unwrap();
    repository
        .reference(
            "refs/remotes/origin/main",
            base.id(),
            false,
            "Fixture reference",
        )
        .unwrap();
    let linked = temp.path().join("linked");
    repository.worktree("linked", &linked, None).unwrap();
    Repository::open(&linked)
        .unwrap()
        .set_head_detached(base.id())
        .unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let main = worktree(&local, &root).await;
    let linked = worktree(&local, &linked).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    let db = rusqlite::Connection::open(node.profile().join("storage/node.sqlite3")).unwrap();
    let count = || {
        db.query_row("SELECT count(*) FROM requests", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap()
    };
    let before = count();
    std::fs::write(root.join("file"), "preserved draft").unwrap();
    let index = std::fs::read(repository.path().join("index")).unwrap();
    for client in [&local, &remote] {
        for revision in [
            "feature/中文",
            "release",
            "origin/main",
            "HEAD~1",
            &base.id().to_string(),
            &base.id().to_string()[..12],
        ] {
            let admission = client
                .dispatch(client.prepare(Command::ResolveGitRevision {
                    worktree: main,
                    revision: revision.into(),
                }))
                .await
                .unwrap();
            assert!(!admission.receipt.durable);
            assert_eq!(
                admission.completion.await.unwrap().unwrap(),
                Output::GitRevision {
                    commit: base.id().to_string()
                }
            );
        }
        for (worktree, expected) in [(main, tip), (linked, base.id())] {
            assert_eq!(
                client
                    .execute(client.prepare(Command::ResolveGitRevision {
                        worktree,
                        revision: "HEAD".into()
                    }))
                    .await
                    .unwrap(),
                Output::GitRevision {
                    commit: expected.to_string()
                }
            );
        }
        for (revision, code) in [
            ("", ErrorCode::InvalidRequest),
            ("missing", ErrorCode::NotFound),
            ("HEAD:file", ErrorCode::InvalidRequest),
            ("HEAD^{tree}", ErrorCode::InvalidRequest),
            ("HEAD\n", ErrorCode::InvalidRequest),
            (&"a".repeat(1025), ErrorCode::InvalidRequest),
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(Command::ResolveGitRevision {
                        worktree: main,
                        revision: revision.into()
                    }))
                    .await
                    .unwrap_err()
                    .code,
                code
            );
        }
        assert_eq!(
            client
                .execute(client.prepare(Command::ResolveGitRevision {
                    worktree: WorktreeId::new(),
                    revision: "HEAD".into()
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }
    assert_eq!(count(), before);
    assert_eq!(repository.head().unwrap().target(), Some(tip));
    assert_eq!(
        std::fs::read(root.join("file")).unwrap(),
        b"preserved draft"
    );
    assert_eq!(
        std::fs::read(repository.path().join("index")).unwrap(),
        index
    );
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
