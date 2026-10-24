use super::*;

#[tokio::test]
async fn resolves_the_admitted_worktree() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    let other = directory.path().join("other");
    let profile = directory.path().join("node");
    for path in [&root, &other, &profile] {
        std::fs::create_dir(path).unwrap();
    }
    let profile = profile.canonicalize().unwrap();
    let mut database = Database::open(&profile.join("node.turso"), NodeId([8; 32]), None)
        .await
        .unwrap();
    let (events, _) = tokio::sync::broadcast::channel(64);
    let node = database.node;
    let (session, turn, alternative) = {
        let mut execute = async |command| {
            let (reply, response) = oneshot::channel();
            assert!(
                database
                    .dispatch(node, Request::new(node, command), reply, &events)
                    .await
                    .is_none()
            );
            response
                .await
                .unwrap()
                .unwrap()
                .completion
                .await
                .unwrap()
                .unwrap()
        };
        let Output::Project(project) = execute(Command::RegisterProject {
            name: "Frozen workspace fixture".into(),
            path: root.to_str().unwrap().into(),
        })
        .await
        else {
            panic!("project expected")
        };
        let Output::Session(session) = execute(Command::CreateSession {
            project: Some(project.id),
            worktree: None,
            config: Some(SessionConfig {
                features: Default::default(),
                assistant: None,
                resource: None,
                provider: ProviderId::new(),
                model: "fixture".into(),
                effort: Effort::Low,
                mode: WorkMode::Code,
                options: Default::default(),
                permission: Permission::Ask,
                credential: None,
            }),
        })
        .await
        else {
            panic!("session expected")
        };
        let Output::QueuedTurn(turn) = execute(Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Run in the admitted workspace".into(),
        })
        .await
        else {
            panic!("turn expected")
        };
        let Output::Project(alternative) = execute(Command::RegisterProject {
            name: "Other fixture".into(),
            path: other.to_str().unwrap().into(),
        })
        .await
        else {
            panic!("project expected")
        };
        (session, turn, alternative)
    };
    database.set_profile(Some(profile)).unwrap();
    let replacement = super::super::worktrees::select(&database.connection, alternative.id, None)
        .await
        .unwrap();
    database
        .connection
        .execute(
            "UPDATE sessions SET worktree=?1 WHERE id=?2",
            turso::params![replacement.to_string(), session.id.to_string()],
        )
        .await
        .unwrap();
    database.connection.execute("UPDATE agent_runs SET body=CAST(json_set(CAST(body AS TEXT),'$.status','running') AS BLOB) WHERE turn=?1", [turn.id.to_string()]).await.unwrap();
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
                pty: false,
                background: false,
                attachments: vec![],
            },
        ),
    )
    .await
    .unwrap();
    assert_eq!(prepared.worktree, session.worktree);
    assert_eq!(prepared.launch.root, root.canonicalize().unwrap());
    assert_ne!(prepared.launch.root, other.canonicalize().unwrap());
    assert_eq!(
        prepared.launch.access,
        sailry_protocol::process::Access::Workspace
    );
}
