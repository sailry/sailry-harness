use super::*;

#[cfg(unix)]
mod cleanup;
#[cfg(unix)]
mod fixture;
#[cfg(unix)]
mod startup;

#[tokio::test]
async fn exits_publish_without_notices() {
    let directory = tempfile::tempdir().unwrap();
    let mut database = Database::open(
        &crate::store::tests::profile(&directory).join("node.turso"),
        NodeId([8; 32]),
        None,
    )
    .await
    .unwrap();
    let (Output::Project(project), _) = super::super::projects::register(
        &database.connection,
        "Exit fixture",
        directory.path().to_str().unwrap(),
        Default::default(),
    )
    .await
    .unwrap() else {
        panic!("project expected")
    };
    let worktree = super::super::worktrees::select(&database.connection, project.id, None)
        .await
        .unwrap();
    let exits = [
        Status::Exited { code: 0 },
        Status::Exited { code: 7 },
        Status::Failed {
            message: "signal".into(),
        },
    ];
    // Local worktree shells and remote SSH shells share the same exit path.
    for (worktree, ssh) in [(Some(worktree), None), (None, Some(SshId::new()))] {
        for exit in &exits {
            let mut info = Info {
                id: TerminalId::new(),
                worktree,
                ssh,
                tool: None,
                access: None,
                status: Status::Running,
                activity: None,
                title: Some("Fixture".into()),
                directory: None,
                owner: None,
                revision: 1,
            };
            record(&database.connection, &info).await.unwrap();
            let (events, mut receiver) = broadcast::channel(16);
            info.status = exit.clone();
            database
                .terminal_changed(info.clone(), &events)
                .await
                .unwrap();
            database
                .terminal_changed(info.clone(), &events)
                .await
                .unwrap();
            assert!(matches!(
                receiver.try_recv().unwrap().event,
                Event::TerminalChanged(changed) if changed.status == *exit
            ));
            assert!(receiver.try_recv().is_err());
            assert_eq!(
                list(&database.connection)
                    .await
                    .unwrap()
                    .iter()
                    .find(|item| item.id == info.id)
                    .unwrap()
                    .status,
                *exit
            );
        }
    }
    assert!(
        super::super::notifications::list(&database.connection)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn ignores_stale_metadata() {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(
        "CREATE TABLE terminals(id TEXT PRIMARY KEY, worktree TEXT, body BLOB NOT NULL)",
    )
    .await
    .unwrap();
    let mut info = Info {
        id: TerminalId::new(),
        worktree: None,
        ssh: Some(SshId::new()),
        tool: None,
        access: None,
        status: Status::Running,
        activity: None,
        title: None,
        directory: None,
        owner: None,
        revision: 1,
    };
    assert!(record(&db, &info).await.unwrap().is_some());
    let opening = info.clone();
    info.title = Some("Server".into());
    info.directory = Some("file://localhost/tmp".into());
    assert!(record_update(&db, &info, true).await.unwrap().is_some());
    assert!(record(&db, &opening).await.unwrap().is_none());
    assert_eq!(list(&db).await.unwrap(), vec![info.clone()]);
    assert!(record_update(&db, &info, true).await.unwrap().is_none());
    info.title = None;
    assert!(record_update(&db, &info, true).await.unwrap().is_some());
    info.revision += 1;
    info.status = Status::Closed;
    assert!(record(&db, &info).await.unwrap().is_some());
    assert!(record_update(&db, &opening, true).await.unwrap().is_none());
}

#[tokio::test]
async fn requires_explicit_relaunch() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(
        &crate::store::tests::profile(&directory).join("node.turso"),
        NodeId([7; 32]),
        None,
    )
    .await
    .unwrap();
    let (Output::Project(project), _) = super::super::projects::register(
        &database.connection,
        "Terminal fixture",
        directory.path().to_str().unwrap(),
        Default::default(),
    )
    .await
    .unwrap() else {
        panic!("project expected")
    };
    let worktree = super::super::worktrees::select(&database.connection, project.id, None)
        .await
        .unwrap();
    for (tool, ssh) in [
        (Some(terminal::Tool::Codex), None),
        (None, Some(SshId::new())),
    ] {
        let info = Info {
            id: TerminalId::new(),
            worktree: ssh.is_none().then_some(worktree),
            ssh,
            tool,
            access: ssh
                .is_none()
                .then_some(sailry_protocol::process::Access::Workspace),
            status: Status::Stopped,
            activity: None,
            title: None,
            directory: None,
            owner: None,
            revision: 2,
        };
        record(&database.connection, &info).await.unwrap();
        assert!(
            matches!(database.terminal_opening(info.id).await, Err(error) if error.code == ErrorCode::Conflict)
        );
    }
}
