use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::*;

async fn search(
    client: &Client,
    worktree: WorktreeId,
    query: &str,
    regex: bool,
    case_sensitive: bool,
    globs: Vec<String>,
) -> Result<SearchResults, Fault> {
    let output = client
        .execute(client.prepare(Command::SearchFiles {
            worktree,
            options: FileSearch {
                query: query.into(),
                regex,
                case_sensitive,
                globs,
            },
        }))
        .await?;
    let Output::SearchResults(result) = output else {
        panic!()
    };
    Ok(result)
}

#[tokio::test]
async fn shares_read_only_search() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir_all(root.join("src/资料")).unwrap();
    std::fs::create_dir(root.join(".git")).unwrap();
    std::fs::write(root.join(".git/private"), "Needle").unwrap();
    std::fs::write(
        root.join("src/资料/中文.rs"),
        "first\r\nNeedle 中文\r\nneedle\n[needle]\n",
    )
    .unwrap();
    std::fs::write(root.join("notes.md"), "Needle in notes\n").unwrap();
    std::fs::write(root.join("binary"), b"Needle\0binary").unwrap();
    std::fs::write(root.join("invalid"), b"Needle\xff").unwrap();
    let external = temp.path().join("outside");
    std::fs::create_dir(&external).unwrap();
    std::fs::write(external.join("secret.txt"), "Needle outside").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&external, root.join("linked")).unwrap();
        std::os::unix::fs::symlink(external.join("secret.txt"), root.join("secret.txt")).unwrap();
    }
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    local
        .execute(local.prepare(Command::RegisterProject {
            name: "Search".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap();
    let Output::Snapshot(snapshot) = local
        .execute(local.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!()
    };
    let worktree = snapshot.worktrees[0].id;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let invitation = node.link().invite().unwrap();
    let address = controller.handle().pair(invitation.ticket()).await.unwrap();
    let transport = controller.handle().remote(address);
    let remote = Client::new(transport.clone());
    for client in [&local, &remote] {
        let all = search(client, worktree, "needle", false, false, vec![])
            .await
            .unwrap();
        assert_eq!(all.matches.len(), 4);
        assert!(all.skipped >= 2);
        assert!(!all.truncated);
        assert!(
            all.matches
                .iter()
                .all(|found| !found.line.contains("outside") && !found.path.contains(".git"))
        );
        let exact = search(
            client,
            worktree,
            "Needle",
            false,
            true,
            vec!["src/**/*.rs".into()],
        )
        .await
        .unwrap();
        assert_eq!(exact.matches.len(), 1);
        assert_eq!(exact.matches[0].line_number, 2);
        assert_eq!(exact.matches[0].path, "src/资料/中文.rs");
        assert_eq!(exact.matches[0].line, "Needle 中文");
        let regular = search(client, worktree, "^needle$", true, false, vec![])
            .await
            .unwrap();
        assert_eq!(regular.matches.len(), 1);
        assert_eq!(regular.matches[0].line_number, 3);
        let literal = search(client, worktree, "[needle]", false, true, vec![])
            .await
            .unwrap();
        assert_eq!(literal.matches.len(), 1);
        assert_eq!(literal.matches[0].line_number, 4);
        assert_eq!(
            search(client, worktree, "[", true, false, vec![])
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            search(client, worktree, "needle", false, false, vec!["[".into()])
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            search(client, worktree, "", false, false, vec![])
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            search(client, WorktreeId::new(), "needle", false, false, vec![])
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert!(
            search(client, worktree, "no such text", false, false, vec![])
                .await
                .unwrap()
                .matches
                .is_empty()
        );
    }
    let admitted = transport
        .dispatch(remote.prepare(Command::SearchFiles {
            worktree,
            options: FileSearch {
                query: "needle".into(),
                regex: false,
                case_sensitive: false,
                globs: vec![],
            },
        }))
        .await
        .unwrap();
    assert!(!admitted.receipt.durable);
    admitted.completion.await.unwrap().unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("src/资料/中文.rs")).unwrap(),
        "first\r\nNeedle 中文\r\nneedle\n[needle]\n"
    );
    let db = rusqlite::Connection::open(node.profile().join("storage/node.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM requests", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn reports_truncation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join("many.txt"),
        "hit\n".repeat(MAX_SEARCH_MATCHES + 1),
    )
    .unwrap();
    std::fs::write(root.join("long.txt"), format!("hit{}", "中文".repeat(2000))).unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let client = Client::new(node.local());
    client
        .execute(client.prepare(Command::RegisterProject {
            name: "Limits".into(),
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
    let id = snapshot.worktrees[0].id;
    let many = search(&client, id, "hit", false, true, vec!["many.txt".into()])
        .await
        .unwrap();
    assert_eq!(many.matches.len(), MAX_SEARCH_MATCHES);
    assert!(many.truncated);
    let long = search(&client, id, "hit", false, true, vec!["long.txt".into()])
        .await
        .unwrap();
    assert!(long.truncated && long.matches[0].line.len() <= 1024);
    node.shutdown().await.unwrap();
}
