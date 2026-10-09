use super::*;

#[test]
fn resolves_the_admitted_worktree() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    let other = directory.path().join("other");
    let profile = directory.path().join("node");
    for path in [&root, &other, &profile] {
        std::fs::create_dir(path).unwrap();
    }
    let profile = profile.canonicalize().unwrap();
    let mut database =
        Database::open(&profile.join("node.sqlite3"), NodeId([8; 32]), None).unwrap();
    let (events, _) = tokio::sync::broadcast::channel(64);
    let node = database.node;
    let (session, turn, alternative) = {
        let mut execute = |command| {
            let (reply, response) = oneshot::channel();
            assert!(
                database
                    .dispatch(node, Request::new(node, command), reply, &events)
                    .is_none()
            );
            response
                .blocking_recv()
                .unwrap()
                .unwrap()
                .completion
                .blocking_recv()
                .unwrap()
                .unwrap()
        };
        let Output::Project(project) = execute(Command::RegisterProject {
            name: "Frozen workspace fixture".into(),
            path: root.to_str().unwrap().into(),
        }) else {
            panic!("project expected")
        };
        let Output::Session(session) = execute(Command::CreateSession {
            project: Some(project.id),
            worktree: None,
            config: Some(SessionConfig {
                assistant: None,
                resource: None,
                provider: ProviderId::new(),
                model: "fixture".into(),
                effort: Effort::Low,
                mode: WorkMode::Code,
                permission: Permission::Ask,
                credential: None,
            }),
        }) else {
            panic!("session expected")
        };
        let Output::QueuedTurn(turn) = execute(Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Run in the admitted workspace".into(),
        }) else {
            panic!("turn expected")
        };
        let Output::Project(alternative) = execute(Command::RegisterProject {
            name: "Other fixture".into(),
            path: other.to_str().unwrap().into(),
        }) else {
            panic!("project expected")
        };
        (session, turn, alternative)
    };
    database.profile = Some(profile);
    let replacement =
        super::super::worktrees::select(&database.connection, alternative.id, None).unwrap();
    database
        .connection
        .execute(
            "UPDATE sessions SET worktree=?1 WHERE id=?2",
            rusqlite::params![replacement.to_string(), session.id.to_string()],
        )
        .unwrap();
    database.connection.execute("UPDATE agent_runs SET body=CAST(json_set(CAST(body AS TEXT),'$.status','running') AS BLOB) WHERE turn=?1", [turn.id.to_string()]).unwrap();
    let prepared = prepare(
        &database,
        &Request::new(
            node,
            Command::RunCommand {
                turn: turn.id,
                command: "true".into(),
                access: sailry_protocol::process::Access::Workspace,
                description: None,
                cwd: String::new(),
                timeout_ms: 1000,
                background: false,
                attachments: vec![],
            },
        ),
    )
    .unwrap();
    assert_eq!(prepared.worktree, session.worktree);
    assert_eq!(prepared.launch.root, root.canonicalize().unwrap());
    assert_ne!(prepared.launch.root, other.canonicalize().unwrap());
    assert_eq!(
        prepared.launch.access,
        sailry_protocol::process::Access::Workspace
    );
}
