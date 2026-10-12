use super::*;

async fn managed() -> (Fixture, Arc<Pool>, PathBuf, code::Receipt) {
    let fixture = Fixture::new().await;
    let profile = fixture._directory.path().join("profile");
    let directory = profile.join("tools/lsp/rust-analyzer/1");
    std::fs::create_dir_all(directory.join("bin")).unwrap();
    std::fs::copy(
        fixture.bin.join("rust-analyzer"),
        directory.join("bin/rust-analyzer"),
    )
    .unwrap();
    let receipt = code::Receipt {
        package: "rust-analyzer".into(),
        version: "1".into(),
        purl: "pkg:github/fixture/server@1".into(),
        revision: "a".repeat(64),
        downloads: Vec::new(),
        installed_at: "2026-01-01T00:00:00Z".into(),
    };
    std::fs::write(
        directory.join("receipt.json"),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();
    let pool = Pool::new(
        Some(profile),
        fixture.environment.clone(),
        CancellationToken::new(),
    );
    (fixture, pool, directory, receipt)
}

async fn start(pool: &Pool, worktree: WorktreeId, root: &Path) -> Startup {
    let startup = pool
        .prepare(
            worktree,
            root.to_owned(),
            &Query::Diagnostics {
                path: "main.rs".into(),
                severity: None,
            },
        )
        .await
        .unwrap()
        .unwrap();
    pool.start(&startup, root, &CancellationToken::new())
        .await
        .unwrap();
    startup
}

#[tokio::test]
async fn preserves_servers_on_invalid_receipt() {
    let (fixture, pool, directory, mut receipt) = managed().await;
    let startup = start(&pool, fixture.worktree, &fixture.root).await;
    assert!(Path::new(&startup.executable).starts_with(directory.canonicalize().unwrap()));
    receipt.version = "other".into();
    std::fs::write(
        directory.join("receipt.json"),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();
    assert_eq!(
        pool.uninstall("rust-analyzer", "1").await.unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert!(directory.join("bin/rust-analyzer").is_file());
    let servers = pool.list(fixture.worktree).await.unwrap();
    assert_eq!(servers.len(), 1);
    assert!(matches!(
        servers[0].status,
        Status::Ready | Status::Indexing
    ));
    assert!(
        pool.state
            .lock()
            .await
            .prepared
            .contains_key(&startup.revision)
    );
    pool.shutdown().await;
}

#[tokio::test]
async fn rejects_shared_use_before_stopping_idle_instances() {
    let (fixture, pool, directory, _) = managed().await;
    let idle = start(&pool, fixture.worktree, &fixture.root).await;
    let other_root = fixture
        ._directory
        .path()
        .canonicalize()
        .unwrap()
        .join("other");
    std::fs::create_dir(&other_root).unwrap();
    for name in ["main.rs", "target.rs", "Cargo.toml"] {
        std::fs::copy(fixture.root.join(name), other_root.join(name)).unwrap();
    }
    let other_worktree = WorktreeId::new();
    let active = start(&pool, other_worktree, &other_root).await;
    std::fs::write(other_root.join("control"), br#"{"hang":true}"#).unwrap();
    let stop = CancellationToken::new();
    let cancel = stop.clone();
    let running = pool.clone();
    let root = other_root.clone();
    let query = tokio::spawn(async move {
        running
            .query(
                other_worktree,
                root,
                &Query::Hover {
                    path: "main.rs".into(),
                    line: 1,
                    symbol: "symbol".into(),
                },
                None,
                &stop,
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let messages = std::fs::read_to_string(other_root.join("messages")).unwrap_or_default();
            if messages
                .lines()
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .any(|message| message["method"] == "textDocument/hover")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        pool.uninstall("rust-analyzer", "1").await.unwrap_err().code,
        ErrorCode::Busy
    );
    assert!(directory.join("bin/rust-analyzer").is_file());
    assert_eq!(pool.list(fixture.worktree).await.unwrap()[0].startup, idle);
    assert_eq!(pool.list(other_worktree).await.unwrap()[0].startup, active);
    cancel.cancel();
    assert_eq!(query.await.unwrap().unwrap_err().code, ErrorCode::Cancelled);
    pool.uninstall("rust-analyzer", "1").await.unwrap();
    assert!(!directory.exists());
    assert!(pool.list(fixture.worktree).await.unwrap().is_empty());
    assert!(pool.list(other_worktree).await.unwrap().is_empty());
    assert!(
        !pool
            .state
            .lock()
            .await
            .prepared
            .contains_key(&idle.revision)
    );
    assert_eq!(
        pool.start(&idle, &fixture.root, &CancellationToken::new())
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert!(fixture.bin.join("rust-analyzer").is_file());
    assert!(fixture.root.join("main.rs").is_file());
    pool.shutdown().await;
}
