use super::*;

#[cfg(unix)]
mod cleanup;
#[cfg(unix)]
mod fixture;
#[cfg(unix)]
mod startup;

#[test]
fn completion_notice_follows_one_metadata_transition() {
    let directory = tempfile::tempdir().unwrap();
    let mut database = Database::open(
        &directory
            .path()
            .canonicalize()
            .unwrap()
            .join("node.sqlite3"),
        NodeId([8; 32]),
        None,
    )
    .unwrap();
    let (Output::Project(project), _) = super::super::projects::register(
        &database.connection,
        "Notification fixture",
        directory.path().to_str().unwrap(),
        Default::default(),
    )
    .unwrap() else {
        panic!("project expected")
    };
    let worktree = super::super::worktrees::select(&database.connection, project.id, None).unwrap();
    let mut info = Info {
        id: TerminalId::new(),
        worktree: Some(worktree),
        ssh: None,
        tool: None,
        access: None,
        status: Status::Running,
        activity: None,
        title: Some("Fixture".into()),
        directory: None,
        owner: None,
        revision: 1,
    };
    record(&database.connection, &info).unwrap();
    let (events, mut receiver) = broadcast::channel(16);
    info.status = Status::Exited { code: 7 };
    database.terminal_changed(info.clone(), &events).unwrap();
    database.terminal_changed(info, &events).unwrap();
    let notices = super::super::notifications::list(&database.connection).unwrap();
    assert_eq!(notices.len(), 1);
    assert!(
        matches!(&notices[0].content.details, Some(notification::Details::Terminal(details)) if details.code == Some(7) && details.failed)
    );
    assert!(matches!(
        receiver.try_recv().unwrap().event,
        Event::TerminalChanged(_)
    ));
    assert!(matches!(
        receiver.try_recv().unwrap().event,
        Event::NotificationChanged(_)
    ));
    assert!(receiver.try_recv().is_err());
}

#[test]
fn ignores_stale_metadata() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(
        "CREATE TABLE terminals(id TEXT PRIMARY KEY, worktree TEXT, body BLOB NOT NULL)",
    )
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
    assert!(record(&db, &info).unwrap().is_some());
    let opening = info.clone();
    info.title = Some("Server".into());
    info.directory = Some("file://localhost/tmp".into());
    assert!(record_update(&db, &info, true).unwrap().is_some());
    assert!(record(&db, &opening).unwrap().is_none());
    assert_eq!(list(&db).unwrap(), vec![info.clone()]);
    assert!(record_update(&db, &info, true).unwrap().is_none());
    info.title = None;
    assert!(record_update(&db, &info, true).unwrap().is_some());
    info.revision += 1;
    info.status = Status::Closed;
    assert!(record(&db, &info).unwrap().is_some());
    assert!(record_update(&db, &opening, true).unwrap().is_none());
}

#[test]
fn requires_explicit_relaunch() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(
        &directory
            .path()
            .canonicalize()
            .unwrap()
            .join("node.sqlite3"),
        NodeId([7; 32]),
        None,
    )
    .unwrap();
    let (Output::Project(project), _) = super::super::projects::register(
        &database.connection,
        "Terminal fixture",
        directory.path().to_str().unwrap(),
        Default::default(),
    )
    .unwrap() else {
        panic!("project expected")
    };
    let worktree = super::super::worktrees::select(&database.connection, project.id, None).unwrap();
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
        record(&database.connection, &info).unwrap();
        assert!(
            matches!(database.terminal_opening(info.id), Err(error) if error.code == ErrorCode::Conflict)
        );
    }
}
