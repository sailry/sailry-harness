use crate::file_fixture::Fixture;
use sailry_protocol::*;

#[tokio::test]
async fn preserves_history_queries_and_cursors() {
    let fixture = Fixture::empty().await;
    let repository = git2::Repository::init(&fixture.root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let mut previous = None;
    for content in ["first\n", "second\n", "third\n"] {
        std::fs::write(fixture.root.join("tracked.txt"), content).unwrap();
        let mut index = repository.index().unwrap();
        index.add_path(std::path::Path::new("tracked.txt")).unwrap();
        index.write().unwrap();
        let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
        let parent = previous.map(|oid| repository.find_commit(oid).unwrap());
        previous = Some(
            repository
                .commit(
                    Some("HEAD"),
                    &signature,
                    &signature,
                    content,
                    &tree,
                    &parent.iter().collect::<Vec<_>>(),
                )
                .unwrap(),
        );
    }
    let [local, remote] = fixture.clients();
    let command = |cursor| Command::ReadGitLog {
        worktree: fixture.worktree,
        limit: 1,
        cursor,
        revision: Some("main".into()),
        path: Some("tracked.txt".into()),
    };
    let mut cursor = None;
    for offset in 0..3 {
        let request = command(cursor);
        let expected = local.execute(local.prepare(request.clone())).await;
        let admission = remote.dispatch(remote.prepare(request)).await.unwrap();
        assert!(!admission.receipt.durable);
        assert_eq!(admission.completion.await.unwrap(), expected);
        let Output::GitLog(page) = expected.unwrap() else {
            panic!("Git history expected")
        };
        assert_eq!(page.offset, offset);
        assert_eq!(page.entries.len(), 1);
        if let Some(cursor) = &page.next {
            assert_eq!(cursor.path.as_deref(), Some("tracked.txt"));
            let mut changed = cursor.clone();
            changed.path = None;
            let changed = command(Some(changed));
            for client in [&local, &remote] {
                assert_eq!(
                    client
                        .execute(client.prepare(changed.clone()))
                        .await
                        .unwrap_err()
                        .code,
                    ErrorCode::InvalidRequest
                );
            }
        }
        cursor = page.next;
    }
    assert!(cursor.is_none());
    let invalid = command(Some(GitLogCursor {
        head: previous.unwrap().to_string(),
        after: "0".repeat(40),
        path: Some("tracked.txt".into()),
    }));
    let expected = local.execute(local.prepare(invalid.clone())).await;
    assert_eq!(remote.execute(remote.prepare(invalid)).await, expected);
    assert_eq!(expected.unwrap_err().code, ErrorCode::InvalidRequest);
    drop(repository);
    fixture.close().await;
}
