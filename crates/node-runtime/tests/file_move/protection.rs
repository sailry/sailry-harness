use super::*;

#[tokio::test]
async fn confines_owned_roots() {
    let fixture = Fixture::start().await;
    let (destination, worktree) = destination(&fixture).await;
    let clients = fixture.clients();
    let enclosing = register(&clients[0], fixture._temp.path()).await;
    std::fs::create_dir(fixture.root.join("nested")).unwrap();
    register(&clients[0], &fixture.root.join("nested")).await;
    std::fs::create_dir(destination.join("nested")).unwrap();
    register(&clients[0], &destination.join("nested")).await;
    std::fs::write(fixture.root.join("file"), "source").unwrap();
    std::fs::write(destination.join("file"), "destination").unwrap();
    for client in &clients {
        for (source, target, from, to, code) in [
            (
                WorktreeId::new(),
                worktree,
                "file",
                "moved",
                ErrorCode::NotFound,
            ),
            (
                fixture.worktree,
                WorktreeId::new(),
                "file",
                "moved",
                ErrorCode::NotFound,
            ),
            (
                fixture.worktree,
                worktree,
                "nested",
                "moved",
                ErrorCode::Conflict,
            ),
            (
                fixture.worktree,
                worktree,
                "file",
                "nested",
                ErrorCode::Conflict,
            ),
            (
                enclosing,
                worktree,
                "node/storage/node.sqlite3",
                "moved",
                ErrorCode::Conflict,
            ),
            (
                fixture.worktree,
                enclosing,
                "file",
                "node/incoming",
                ErrorCode::Conflict,
            ),
            (
                fixture.worktree,
                worktree,
                "../file",
                "moved",
                ErrorCode::InvalidRequest,
            ),
            (
                fixture.worktree,
                worktree,
                "file",
                "../moved",
                ErrorCode::InvalidRequest,
            ),
            (
                fixture.worktree,
                worktree,
                ".git/config",
                "moved",
                ErrorCode::PermissionDenied,
            ),
            (
                fixture.worktree,
                worktree,
                "file",
                ".git/config",
                ErrorCode::PermissionDenied,
            ),
            (
                fixture.worktree,
                worktree,
                "file",
                "file",
                ErrorCode::Conflict,
            ),
            (
                fixture.worktree,
                worktree,
                "",
                "moved",
                ErrorCode::InvalidRequest,
            ),
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(move_to(source, target, from, to)))
                    .await
                    .unwrap_err()
                    .code,
                code
            );
        }
        let mut wrong = client.prepare(move_to(fixture.worktree, worktree, "file", "moved"));
        wrong.target = NodeId([0; 32]);
        assert_eq!(
            client.execute(wrong).await.unwrap_err().code,
            ErrorCode::WrongTarget
        );
    }
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("file")).unwrap(),
        "source"
    );
    assert_eq!(
        std::fs::read_to_string(destination.join("file")).unwrap(),
        "destination"
    );
    assert!(!destination.join("moved").exists());
    fixture.close().await;
}
