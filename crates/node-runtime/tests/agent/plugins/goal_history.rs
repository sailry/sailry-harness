#![cfg(unix)]
use super::*;
use process::Fixture;
use sailry_protocol::plugin;
use serde_json::{Value, json};

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

async fn fixture(remote: bool, server: &Server) -> Fixture {
    let fixture = Fixture::new(remote, server).await;
    let Output::Plugin(package) = execute(
        &fixture.client,
        Command::ReadPlugin {
            name: "goals".into(),
        },
    )
    .await
    else {
        panic!("goals package expected")
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    if !package.summary.enabled {
        execute(
            &fixture.client,
            Command::SetPluginEnabled {
                name: "goals".into(),
                expected_revision: package.summary.revision,
                enabled: true,
            },
        )
        .await;
    }
    fixture
}

async fn context(client: &Client, session: &Session) -> plugin::Context {
    let Output::Plugin(package) = execute(
        client,
        Command::ReadPlugin {
            name: "goals".into(),
        },
    )
    .await
    else {
        panic!("goals package expected")
    };
    plugin::Context {
        invocation: None,
        turn: None,
        surface: plugin::desktop::Surface::Workspace,
        package: package.summary.reference(),
        worktree: Some(session.worktree),
        session: Some(session.id),
    }
}

async fn read(client: &Client, session: &Session) -> Value {
    let request = client
        .prepare(Command::CallPlugin {
            handler: "read".into(),
            input: json!({}),
        })
        .with_plugin(context(client, session).await);
    let Output::PluginResult(value) = client.execute(request).await.unwrap() else {
        panic!("goal result expected")
    };
    value["goal"].clone()
}

async fn entry(client: &Client, session: &Session) -> Value {
    let request = client
        .prepare(Command::ReadPluginConversationValue { key: "goal".into() })
        .with_plugin(context(client, session).await);
    let Output::PluginConversationValue(entry) = client.execute(request).await.unwrap() else {
        panic!("conversation value expected")
    };
    serde_json::to_value(entry).unwrap()
}

async fn input(client: &Client, session: &Session, text: &str) -> Output {
    execute(
        client,
        Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: text.into(),
        },
    )
    .await
}

async fn submit(client: &Client, session: &Session, text: &str) -> TurnId {
    let Output::QueuedTurn(turn) = input(client, session, text).await else {
        panic!("turn expected")
    };
    turn.id
}

async fn settled(client: &Client, session: &Session, state: &str) -> (Value, Page) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let goal = read(client, session).await;
            let page = history(client, session.id).await;
            if goal["state"] == state
                && page.queue.items.is_empty()
                && page.runs.iter().all(|run| {
                    !matches!(
                        run.status,
                        Status::Queued | Status::Running | Status::Stopping
                    )
                })
            {
                return (goal, page);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("goal completion deadline")
}

async fn fork(client: &Client, session: &Session, through: TurnId) -> Session {
    let Output::Session(branch) = execute(
        client,
        Command::ForkConversation {
            session: session.id,
            through,
            expected_revision: session.revision,
        },
    )
    .await
    else {
        panic!("fork expected")
    };
    branch
}

async fn rewind(
    client: &Client,
    session: &Session,
    page: &Page,
    through: Option<TurnId>,
) -> Rewind {
    let Output::Rewound(result) = execute(
        client,
        Command::RewindConversation {
            session: session.id,
            through,
            expected_head: page.runs.last().unwrap().turn,
            expected_revision: page.revision,
        },
    )
    .await
    else {
        panic!("rewind expected")
    };
    *result
}

#[tokio::test]
async fn copies_visible_prefixes_independently() {
    for remote in [false, true] {
        let server = Server::goal_reasoning(2, "completed").await;
        let fixture = fixture(remote, &server).await;
        let first = submit(
            &fixture.client,
            &fixture.session,
            "/goal Original objective 中文 🙂",
        )
        .await;
        let (original, page) = settled(&fixture.client, &fixture.session, "completed").await;
        assert_eq!(page.runs.len(), 2);
        assert!(page.runs.iter().all(|run| run.status == Status::Completed));
        let requests = server.requests.lock().unwrap().len();

        let prefix = fork(&fixture.client, &fixture.session, first).await;
        let terminal = fork(
            &fixture.client,
            &fixture.session,
            page.runs.last().unwrap().turn,
        )
        .await;
        let nested = fork(&fixture.client, &prefix, first).await;
        for branch in [&prefix, &nested] {
            let stored = entry(&fixture.client, branch).await;
            let goal = read(&fixture.client, branch).await;
            assert_eq!(stored["key"], "goal");
            assert_eq!(stored["revision"], 1);
            assert_eq!(stored["present"], true);
            assert_eq!(stored["restored"], true);
            assert_eq!(stored["value"]["state"], "active");
            assert_eq!(goal["id"], original["id"]);
            assert_eq!(goal["description"], "Original objective 中文 🙂");
            assert_eq!(goal["state"], "paused");
            assert_eq!(history(&fixture.client, branch.id).await.runs.len(), 1);
        }
        let goal = read(&fixture.client, &terminal).await;
        assert_eq!(goal["id"], original["id"]);
        assert_eq!(goal["description"], original["description"]);
        assert_eq!(goal["state"], "completed");
        assert_eq!(entry(&fixture.client, &terminal).await["restored"], true);
        assert_eq!(server.requests.lock().unwrap().len(), requests);

        input(&fixture.client, &fixture.session, "/goal clear").await;
        submit(
            &fixture.client,
            &fixture.session,
            "/goal Replacement objective",
        )
        .await;
        let (replacement, _) = settled(&fixture.client, &fixture.session, "completed").await;
        assert_ne!(replacement["id"], original["id"]);
        assert_eq!(replacement["description"], "Replacement objective");
        for branch in [&prefix, &nested, &terminal] {
            let goal = read(&fixture.client, branch).await;
            assert_eq!(goal["id"], original["id"]);
            assert_eq!(goal["description"], original["description"]);
        }
        assert_eq!(read(&fixture.client, &prefix).await["state"], "paused");
        assert_eq!(read(&fixture.client, &nested).await["state"], "paused");
        assert_eq!(read(&fixture.client, &terminal).await["state"], "completed");
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn restores_values_and_preserves_terminal_backups() {
    for remote in [false, true] {
        let server = Server::goal_reasoning(2, "completed").await;
        let fixture = fixture(remote, &server).await;
        let first = submit(
            &fixture.client,
            &fixture.session,
            "/goal Retained objective",
        )
        .await;
        let (original, page) = settled(&fixture.client, &fixture.session, "completed").await;
        let previous = entry(&fixture.client, &fixture.session).await;
        let requests = server.requests.lock().unwrap().len();
        let retained = rewind(&fixture.client, &fixture.session, &page, Some(first)).await;
        let restored = entry(&fixture.client, &fixture.session).await;
        assert!(restored["revision"].as_u64().unwrap() > previous["revision"].as_u64().unwrap());
        assert_eq!(restored["present"], true);
        assert_eq!(restored["restored"], true);
        assert_eq!(restored["value"]["state"], "active");
        assert_eq!(
            read(&fixture.client, &fixture.session).await["state"],
            "paused"
        );
        let backup = read(&fixture.client, &retained.backup).await;
        assert_eq!(backup["id"], original["id"]);
        assert_eq!(backup["description"], original["description"]);
        assert_eq!(backup["state"], "completed");
        assert_eq!(
            entry(&fixture.client, &retained.backup).await["restored"],
            true
        );
        assert_eq!(
            history(&fixture.client, retained.backup.id).await.entries,
            page.entries
        );

        let prefix = history(&fixture.client, fixture.session.id).await;
        assert_eq!(prefix.runs.len(), 1);
        let removed = rewind(&fixture.client, &fixture.session, &prefix, None).await;
        let empty = entry(&fixture.client, &fixture.session).await;
        assert!(empty["revision"].as_u64().unwrap() > restored["revision"].as_u64().unwrap());
        assert_eq!(empty["present"], false);
        assert_eq!(empty["restored"], true);
        assert!(empty["value"].is_null());
        assert!(read(&fixture.client, &fixture.session).await.is_null());
        assert!(
            history(&fixture.client, fixture.session.id)
                .await
                .runs
                .is_empty()
        );
        assert_eq!(
            read(&fixture.client, &removed.backup).await["state"],
            "paused"
        );
        assert_eq!(
            read(&fixture.client, &retained.backup).await["state"],
            "completed"
        );
        assert_eq!(server.requests.lock().unwrap().len(), requests);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn requires_explicit_resume_after_restart() {
    for remote in [false, true] {
        let server = Server::goal_reasoning(2, "completed").await;
        let mut fixture = fixture(remote, &server).await;
        let first = submit(
            &fixture.client,
            &fixture.session,
            "/goal Resume this objective",
        )
        .await;
        let (original, _) = settled(&fixture.client, &fixture.session, "completed").await;
        let branch = fork(&fixture.client, &fixture.session, first).await;
        let restored = entry(&fixture.client, &branch).await;
        let mut requests = server.requests.lock().unwrap().len();

        for restart in [false, true] {
            if restart {
                let profile = fixture.node.profile().to_owned();
                fixture.node.shutdown().await.unwrap();
                fixture.node = Node::start(profile).await.unwrap();
                fixture.client = Client::new(if remote {
                    fixture
                        .controller
                        .handle()
                        .remote(fixture.node.link().address())
                } else {
                    fixture.node.local()
                });
                assert_eq!(entry(&fixture.client, &branch).await, restored);
            }
            let before = history(&fixture.client, branch.id).await;
            let ordinary =
                submit(&fixture.client, &branch, "Reply without resuming the goal").await;
            let page = finished(&fixture.client, branch.id, ordinary).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
            assert_eq!(page.runs.len(), before.runs.len() + 1);
            assert!(page.queue.items.is_empty());
            assert_eq!(read(&fixture.client, &branch).await["state"], "paused");
            assert_eq!(entry(&fixture.client, &branch).await, restored);
            requests += 1;
            assert_eq!(server.requests.lock().unwrap().len(), requests);
            let request = server.requests.lock().unwrap().last().unwrap().clone();
            assert!(
                !request["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|message| {
                        message["content"]
                            .as_str()
                            .is_some_and(|text| text.contains("Active goal: "))
                    })
            );
        }

        let before = history(&fixture.client, branch.id).await;
        input(&fixture.client, &branch, "/goal resume").await;
        let (resumed, page) = settled(&fixture.client, &branch, "completed").await;
        assert_eq!(resumed["id"], original["id"]);
        assert_eq!(resumed["description"], original["description"]);
        assert_eq!(page.runs.len(), before.runs.len() + 2);
        assert!(page.runs.iter().all(|run| run.status == Status::Completed));
        let current = entry(&fixture.client, &branch).await;
        assert_eq!(current["restored"], false);
        assert!(current["revision"].as_u64().unwrap() > restored["revision"].as_u64().unwrap());
        assert_eq!(server.requests.lock().unwrap().len(), requests + 4);
        assert_eq!(
            read(&fixture.client, &fixture.session).await["state"],
            "completed"
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
