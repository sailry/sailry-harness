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
                follow_up: false,
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

#[cfg(unix)]
mod promotion {
    use super::super::*;
    use sailry_protocol::process::{Access, Outcome, OutputStream, Status};
    use std::time::Duration;

    struct Fixture {
        root: tempfile::TempDir,
        profile: tempfile::TempDir,
        commands: Arc<process::background::Commands>,
        finished: mpsc::UnboundedReceiver<process::background::Finished>,
        session: SessionId,
        id: RequestId,
        stop: CancellationToken,
    }

    impl Fixture {
        fn new() -> Self {
            let commands = Arc::new(process::background::Commands::default());
            let (sink, finished) = mpsc::unbounded_channel();
            commands.set_sink(sink);
            Self {
                root: tempfile::tempdir().unwrap(),
                profile: tempfile::tempdir().unwrap(),
                commands,
                finished,
                session: SessionId::new(),
                id: RequestId::new(),
                stop: CancellationToken::new(),
            }
        }

        async fn run(
            &self,
            command: &str,
            promotion: impl std::future::Future<Output = ()>,
        ) -> Result<Output, Fault> {
            let launch = Launch {
                profile: Some(self.profile.path().to_owned()),
                root: self.root.path().canonicalize().unwrap(),
                command: command.into(),
                access: Access::Workspace,
                pty: false,
                cwd: String::new(),
                timeout_ms: 30_000,
                inputs: None,
                scratch: None,
                python: None,
            };
            let retained = self
                .commands
                .retain(self.id, self.session, TurnId::new(), &launch);
            let (open, gate) = oneshot::channel();
            let _ = open.send(());
            foreground(
                launch,
                self.id,
                Arc::new(Environment::ambient()),
                self.commands.clone(),
                retained,
                self.stop.clone(),
                CancellationToken::new(),
                promotion,
                gate,
            )
            .await
        }
    }

    #[tokio::test]
    async fn completes_without_guidance() {
        let fixture = Fixture::new();
        let output = fixture
            .run("printf ok", std::future::pending())
            .await
            .unwrap();
        let Output::CommandResult(completion) = output else {
            panic!("foreground command was promoted: {output:?}");
        };
        assert_eq!(completion.outcome, Outcome::Exited(0));
        assert_eq!(completion.stdout.text, "ok");
        assert!(fixture.commands.list(fixture.session).is_empty());
    }

    #[tokio::test]
    async fn stop_before_guidance_cancels() {
        let fixture = Fixture::new();
        let stop = fixture.stop.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            stop.cancel();
        });
        let output = fixture
            .run("sleep 30", std::future::pending())
            .await
            .unwrap();
        let Output::CommandResult(completion) = output else {
            panic!("foreground command was promoted: {output:?}");
        };
        assert_eq!(completion.outcome, Outcome::Cancelled);
    }

    #[tokio::test]
    async fn promotes_running_command_on_guidance() {
        let mut fixture = Fixture::new();
        let output = fixture
            .run(
                "printf started; sleep 2; printf done",
                tokio::time::sleep(Duration::from_millis(500)),
            )
            .await
            .unwrap();
        let Output::CommandStarted(info) = output else {
            panic!("command was not promoted: {output:?}");
        };
        assert_eq!(info.id, fixture.id);
        assert_eq!(info.status, Status::Running);
        let listed = fixture.commands.list(fixture.session);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, fixture.id);
        // The turn's tool stop no longer owns the promoted process.
        fixture.stop.cancel();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let page = fixture
                    .commands
                    .page(fixture.session, fixture.id, OutputStream::Stdout, 0, 1024)
                    .unwrap();
                if page.text == "started" {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("promoted output is not readable");
        let finished = tokio::time::timeout(Duration::from_secs(10), fixture.finished.recv())
            .await
            .expect("promoted completion was not reported")
            .unwrap();
        assert_eq!(finished.snapshot.info.id, fixture.id);
        assert_eq!(
            finished.snapshot.info.status,
            Status::Finished(Outcome::Exited(0))
        );
        assert_eq!(finished.snapshot.stdout.text, "starteddone");
        assert!(!finished.explicit);
        fixture.commands.close().await;
    }

    #[tokio::test]
    async fn promoted_command_stops_explicitly() {
        let mut fixture = Fixture::new();
        let output = fixture
            .run("sleep 30", tokio::time::sleep(Duration::from_millis(200)))
            .await
            .unwrap();
        assert!(matches!(output, Output::CommandStarted(_)));
        fixture.commands.stop(fixture.session, fixture.id).unwrap();
        let finished = tokio::time::timeout(Duration::from_secs(10), fixture.finished.recv())
            .await
            .expect("stopped command did not finish")
            .unwrap();
        assert_eq!(
            finished.snapshot.info.status,
            Status::Finished(Outcome::Cancelled)
        );
        assert!(finished.explicit);
    }
}
