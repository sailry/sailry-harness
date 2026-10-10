#![cfg(unix)]
use super::*;
use crate::code::tests::Fixture;
use serde_json::json;
#[tokio::test]
async fn requires_prepared_descriptors_and_reuses_live_servers() {
    let fixture = Fixture::new().await;
    let closed = CancellationToken::new();
    let pool = Pool::new(None, fixture.environment.clone(), closed.clone());
    let query = Query::Diagnostics {
        path: "main.rs".into(),
        severity: None,
    };
    let stop = CancellationToken::new();
    assert!(
        pool.changed(fixture.worktree, &["main.rs".into()], &stop)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(!fixture.root.join("pid").exists());
    let startup = pool
        .prepare(fixture.worktree, fixture.root.clone(), &query)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pool.identities.reads(), 1);
    let mut forged = startup.clone();
    forged.executable = "/bin/sh".into();
    assert_eq!(
        pool.start(&forged, &fixture.root, &stop)
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert!(!fixture.root.join("pid").exists());
    pool.start(&startup, &fixture.root, &stop).await.unwrap();
    let pid = std::fs::read_to_string(fixture.root.join("pid")).unwrap();
    assert!(
        pool.prepare(fixture.worktree, fixture.root.clone(), &query)
            .await
            .unwrap()
            .is_none()
    );
    pool.start(&startup, &fixture.root, &stop).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("pid")).unwrap(),
        pid
    );
    for _ in 0..3 {
        let result = pool
            .query(
                fixture.worktree,
                fixture.root.clone(),
                &Query::Hover {
                    path: "main.rs".into(),
                    line: 1,
                    symbol: "symbol".into(),
                },
                None,
                &stop,
            )
            .await
            .unwrap();
        assert!(result.startup.is_none());
    }
    assert_eq!(pool.identities.reads(), 1);
    pool.remove(fixture.worktree).await;
    assert!(pool.list(fixture.worktree).await.unwrap().is_empty());
    closed.cancel();
    pool.shutdown().await;
}
#[tokio::test]
async fn checks_executable_changes_before_spawning() {
    let fixture = Fixture::new().await;
    let pool = Pool::new(None, fixture.environment.clone(), CancellationToken::new());
    let query = Query::Diagnostics {
        path: "main.rs".into(),
        severity: None,
    };
    let startup = pool
        .prepare(fixture.worktree, fixture.root.clone(), &query)
        .await
        .unwrap()
        .unwrap();
    let executable = fixture.bin.join("rust-analyzer");
    let mut bytes = std::fs::read(&executable).unwrap();
    bytes.extend_from_slice(b"\n# changed\n");
    std::fs::write(executable, bytes).unwrap();
    assert_eq!(
        pool.start(&startup, &fixture.root, &CancellationToken::new())
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    assert!(!fixture.root.join("pid").exists());
    assert_eq!(pool.identities.reads(), 2);
    pool.shutdown().await;
}
#[tokio::test]
async fn reclaims_idle_servers_and_caps_crash_restarts() {
    let fixture = Fixture::new().await;
    let pool = Pool::new(None, fixture.environment.clone(), CancellationToken::new());
    let query = Query::Diagnostics {
        path: "main.rs".into(),
        severity: None,
    };
    let stop = CancellationToken::new();
    let startup = pool
        .prepare(fixture.worktree, fixture.root.clone(), &query)
        .await
        .unwrap()
        .unwrap();
    pool.start(&startup, &fixture.root, &stop).await.unwrap();
    {
        let state = pool.state.lock().await;
        let server = &state.slots[&key(&startup)].server;
        *server.used.lock().await = Instant::now() - IDLE;
    }
    pool.reap().await;
    assert!(pool.list(fixture.worktree).await.unwrap().is_empty());
    pool.start(&startup, &fixture.root, &stop).await.unwrap();
    fixture.settings(json!({"crash":true}));
    for _ in 0..3 {
        let result = pool
            .query(
                fixture.worktree,
                fixture.root.clone(),
                &Query::Hover {
                    path: "main.rs".into(),
                    line: 1,
                    symbol: "symbol".into(),
                },
                None,
                &stop,
            )
            .await;
        assert!(result.is_err());
        wait_failed(&pool, &startup).await;
        pool.start(&startup, &fixture.root, &stop).await.unwrap();
    }
    assert!(
        pool.query(
            fixture.worktree,
            fixture.root.clone(),
            &Query::Hover {
                path: "main.rs".into(),
                line: 1,
                symbol: "symbol".into()
            },
            None,
            &stop
        )
        .await
        .is_err()
    );
    assert_eq!(
        pool.start(&startup, &fixture.root, &stop)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Unavailable
    );
    pool.shutdown().await;
}

async fn wait_failed(pool: &Pool, startup: &Startup) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let state = pool.state.lock().await;
            let status = state.slots[&key(startup)]
                .server
                .rpc
                .state
                .lock()
                .await
                .status;
            drop(state);
            if status == Status::Failed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn limits_servers_without_evicting_active_queries() {
    let fixture = Fixture::new().await;
    let pool = Pool::new(None, fixture.environment.clone(), CancellationToken::new());
    let stop = CancellationToken::new();
    let query = Query::Diagnostics {
        path: "main.rs".into(),
        severity: None,
    };
    let mut held = Vec::new();
    for _ in 0..CAPACITY {
        let startup = pool
            .prepare(WorktreeId::new(), fixture.root.clone(), &query)
            .await
            .unwrap()
            .unwrap();
        pool.start(&startup, &fixture.root, &stop).await.unwrap();
        held.push(pool.state.lock().await.slots[&key(&startup)].server.clone());
    }
    let startup = pool
        .prepare(WorktreeId::new(), fixture.root.clone(), &query)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        pool.start(&startup, &fixture.root, &stop)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    assert_eq!(pool.state.lock().await.slots.len(), CAPACITY);
    held.clear();
    pool.start(&startup, &fixture.root, &stop).await.unwrap();
    assert_eq!(pool.state.lock().await.slots.len(), CAPACITY);
    pool.shutdown().await;
}

#[tokio::test]
async fn caps_failed_initializations() {
    let fixture = Fixture::new().await;
    fixture.settings(json!({"initial_crash":true}));
    let pool = Pool::new(None, fixture.environment.clone(), CancellationToken::new());
    let startup = pool
        .prepare(
            fixture.worktree,
            fixture.root.clone(),
            &Query::Diagnostics {
                path: "main.rs".into(),
                severity: None,
            },
        )
        .await
        .unwrap()
        .unwrap();
    let stop = CancellationToken::new();
    for _ in 0..4 {
        assert!(pool.start(&startup, &fixture.root, &stop).await.is_err());
    }
    fixture.settings(json!({}));
    assert_eq!(
        pool.start(&startup, &fixture.root, &stop)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Unavailable
    );
    pool.shutdown().await;
}

#[tokio::test]
async fn publishes_diagnostic_changes_without_query_feedback() {
    let fixture = Fixture::new().await;
    let closed = CancellationToken::new();
    let pool = Pool::new(None, fixture.environment.clone(), closed.clone());
    let stop = CancellationToken::new();
    let query = Query::Diagnostics {
        path: "main.rs".into(),
        severity: None,
    };
    let startup = pool
        .prepare(fixture.worktree, fixture.root.clone(), &query)
        .await
        .unwrap()
        .unwrap();
    pool.start(&startup, &fixture.root, &stop).await.unwrap();
    pool.query(fixture.worktree, fixture.root.clone(), &query, None, &stop)
        .await
        .unwrap();
    let mut events = pool.subscribe(
        sailry_protocol::NodeId([1; 32]),
        fixture.worktree,
        closed.clone(),
    );
    let sailry_protocol::Update::CodeServers { servers, .. } = events.next().await.unwrap() else {
        panic!("code update expected")
    };
    let revision = servers[0].diagnostics_revision;
    assert!(revision > 0);
    pool.query(fixture.worktree, fixture.root.clone(), &query, None, &stop)
        .await
        .unwrap();
    assert_eq!(
        pool.list(fixture.worktree).await.unwrap()[0].diagnostics_revision,
        revision
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(400), events.next())
            .await
            .is_err()
    );
    std::fs::write(fixture.root.join("main.rs"), "valid symbol\n").unwrap();
    pool.changed(fixture.worktree, &["main.rs".into()], &stop)
        .await
        .unwrap();
    let sailry_protocol::Update::CodeServers { servers, .. } =
        tokio::time::timeout(Duration::from_secs(3), events.next())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("code update expected")
    };
    assert!(servers[0].diagnostics_revision > revision);
    closed.cancel();
    pool.shutdown().await;
}

#[tokio::test]
async fn freezes_the_returned_descriptor_after_configuration_changes() {
    let fixture = Fixture::new().await;
    let pool = Pool::new(None, fixture.environment.clone(), CancellationToken::new());
    let query = Query::Diagnostics {
        path: "main.rs".into(),
        severity: None,
    };
    let original = pool
        .prepare(fixture.worktree, fixture.root.clone(), &query)
        .await
        .unwrap()
        .unwrap();
    let mut settings = code::Settings::default();
    settings.servers.insert(
        "rust-analyzer".into(),
        code::Override {
            disabled: false,
            command: None,
            args: None,
            config: Some(json!({"fixture":{"option":"captured"}})),
            environment: None,
        },
    );
    pool.configure(settings.clone()).await;
    let stop = CancellationToken::new();
    let result = pool
        .query(fixture.worktree, fixture.root.clone(), &query, None, &stop)
        .await
        .unwrap();
    let returned = result.startup.unwrap();
    assert_ne!(returned.revision, original.revision);
    assert!(!fixture.root.join("pid").exists());
    pool.validate(&returned, &fixture.root).await.unwrap();
    pool.validate(&original, &fixture.root).await.unwrap();
    let mut forged = returned.clone();
    forged.executable = "/bin/sh".into();
    assert_eq!(
        pool.start(&forged, &fixture.root, &stop)
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert!(!fixture.root.join("pid").exists());
    settings.servers.get_mut("rust-analyzer").unwrap().config =
        Some(json!({"fixture":{"option":"later"}}));
    pool.configure(settings).await;
    pool.start(&returned, &fixture.root, &stop).await.unwrap();
    {
        let state = pool.state.lock().await;
        let server = &state.slots[&key(&returned)].server;
        assert_eq!(server.plan.startup, returned);
        assert_eq!(server.plan.config, json!({"fixture":{"option":"captured"}}));
    }
    pool.shutdown().await;
}

#[tokio::test]
async fn invalidates_prepared_work_across_disable_and_enable() {
    let fixture = Fixture::new().await;
    let pool = Pool::new(None, fixture.environment.clone(), CancellationToken::new());
    let query = Query::Hover {
        path: "main.rs".into(),
        line: 1,
        symbol: "symbol".into(),
    };
    let startup = pool
        .prepare(fixture.worktree, fixture.root.clone(), &query)
        .await
        .unwrap()
        .unwrap();
    let (_, captured) = pool.capture().unwrap();
    let mut settings = pool.settings();
    settings.enabled = false;
    settings.after_edit = false;
    pool.configure(settings.clone()).await;
    assert_eq!(
        pool.validate(&startup, &fixture.root).await.unwrap_err(),
        disabled()
    );
    assert!(!fixture.root.join("pid").exists());
    assert_eq!(
        pool.prepare(
            fixture.worktree,
            fixture.root.clone(),
            &Query::Symbols {
                path: None,
                query: None
            }
        )
        .await
        .unwrap_err(),
        disabled()
    );
    assert!(
        pool.changed(
            fixture.worktree,
            &["missing.rs".into()],
            &CancellationToken::new()
        )
        .await
        .unwrap()
        .is_empty()
    );
    assert!(!pool.after_edit());
    settings.enabled = true;
    pool.configure(settings).await;
    assert!(!pool.after_edit());
    assert_eq!(
        pool.current(&captured).unwrap_err().code,
        ErrorCode::Unavailable
    );
    assert_eq!(
        pool.start(&startup, &fixture.root, &CancellationToken::new())
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    let startup = pool
        .prepare(fixture.worktree, fixture.root.clone(), &query)
        .await
        .unwrap()
        .unwrap();
    pool.start(&startup, &fixture.root, &CancellationToken::new())
        .await
        .unwrap();
    let server = pool.state.lock().await.slots[&key(&startup)].server.clone();
    let mut settings = pool.settings();
    settings.enabled = false;
    pool.configure(settings).await;
    assert_eq!(server.rpc.state.lock().await.status, Status::Stopped);
    assert!(pool.list(fixture.worktree).await.unwrap().is_empty());
    assert_eq!(
        pool.query(
            fixture.worktree,
            fixture.root.clone(),
            &query,
            None,
            &CancellationToken::new()
        )
        .await
        .unwrap_err(),
        disabled()
    );
    pool.shutdown().await;
}

#[tokio::test]
async fn cancels_initialization_before_disable_completes() {
    let fixture = Fixture::new().await;
    fixture.settings(json!({"initial_hang":true}));
    let pool = Pool::new(None, fixture.environment.clone(), CancellationToken::new());
    let startup = pool
        .prepare(
            fixture.worktree,
            fixture.root.clone(),
            &Query::Diagnostics {
                path: "main.rs".into(),
                severity: None,
            },
        )
        .await
        .unwrap()
        .unwrap();
    let starting = pool.clone();
    let root = fixture.root.clone();
    let task = tokio::spawn(async move {
        starting
            .start(&startup, &root, &CancellationToken::new())
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while !fixture.root.join("pid").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let mut settings = pool.settings();
    settings.enabled = false;
    tokio::time::timeout(Duration::from_secs(2), pool.configure(settings))
        .await
        .unwrap();
    assert_eq!(task.await.unwrap().unwrap_err(), disabled());
    assert!(pool.list(fixture.worktree).await.unwrap().is_empty());
    assert!(pool.state.lock().await.prepared.is_empty());
    pool.shutdown().await;
}

#[tokio::test]
async fn rejects_stop_after_registered_root_changes() {
    let fixture = Fixture::new().await;
    let pool = Pool::new(None, fixture.environment.clone(), CancellationToken::new());
    let startup = pool
        .prepare(
            fixture.worktree,
            fixture.root.clone(),
            &Query::Diagnostics {
                path: "main.rs".into(),
                severity: None,
            },
        )
        .await
        .unwrap()
        .unwrap();
    pool.start(&startup, &fixture.root, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        pool.stop(&startup, fixture.root.parent().unwrap())
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    let servers = pool.list(fixture.worktree).await.unwrap();
    assert_eq!(servers.len(), 1);
    assert!(matches!(
        servers[0].status,
        Status::Ready | Status::Indexing
    ));
    assert_eq!(
        pool.stop(&startup, &fixture.root).await.unwrap().status,
        Status::Stopped
    );
    assert!(pool.list(fixture.worktree).await.unwrap().is_empty());
    pool.shutdown().await;
}
