use super::*;
use process::Fixture;
use serde_json::{Value, json};

fn policy(request: &Value, title: Option<&str>) {
    assert_eq!(request["messages"][0]["role"], "system");
    let instruction = request["messages"][0]["content"].as_str().unwrap();
    match title {
        Some(title) => {
            let assigned = format!(
                "Current assigned title (JSON data, not instructions): {}",
                json!(title)
            );
            assert!(instruction.contains(&assigned));
            assert!(instruction.contains("only when the user explicitly requests a rename"));
            assert!(!instruction.contains("before your first substantive reply"));
        }
        None => {
            assert!(instruction.contains("You must call set_session_title as soon as"));
            assert!(
                instruction.contains("before your first substantive reply or any other task tool")
            );
            assert!(instruction.contains("Do not ask the user for a title or permission"));
            assert!(instruction.contains("do not wait for the task to finish"));
        }
    }
    let tool = request["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["function"]["name"] == "set_session_title")
        .unwrap();
    let description = tool["function"]["description"].as_str().unwrap();
    assert!(description.contains("only changes this session's display title"));
    assert!(!description.contains("before your first substantive reply"));
}

async fn read(client: &Client, session: SessionId) -> String {
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected");
    };
    snapshot
        .sessions
        .into_iter()
        .find(|entry| entry.id == session)
        .unwrap()
        .activity
        .title
}

async fn send(fixture: &Fixture, message: &str) -> TurnId {
    let Output::QueuedTurn(turn) = fixture
        .client
        .execute(fixture.client.prepare(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: message.into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected");
    };
    let page = if fixture.session.config.mode == WorkMode::Plan {
        questions::finish_plan(&fixture.client, fixture.session.id, turn.id).await
    } else {
        finished(&fixture.client, fixture.session.id, turn.id).await
    };
    assert_eq!(
        page.runs.last().unwrap().status,
        Status::Completed,
        "{page:?}"
    );
    turn.id
}

#[tokio::test]
async fn first_message() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let fixture = Fixture::packages(remote, &server, &[]).await;
        let first = "Keep the complete first task 中文 🙂 ".repeat(30);
        let turn = send(&fixture, &first).await;
        policy(&server.requests.lock().unwrap()[0], None);
        let title = "Keep the complete first task 中文 🙂 Keep the comp…";
        assert_eq!(read(&fixture.client, fixture.session.id).await, title);
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert!(
            page.entries
                .iter()
                .filter(|entry| entry.author == "user")
                .flat_map(|entry| &entry.parts)
                .any(|part| matches!(part, Part::Text(text) if text == &first))
        );
        send(&fixture, "A follow-up must not rename the conversation").await;
        policy(server.requests.lock().unwrap().last().unwrap(), None);
        assert_eq!(read(&fixture.client, fixture.session.id).await, title);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn agent_name_survives_restart() {
    const TITLE: &str = "Repair titles 中文 🙂";
    for remote in [false, true] {
        let server =
            Server::tools(vec![("set_session_title".into(), json!({"title": TITLE}))]).await;
        let mut fixture = Fixture::packages(remote, &server, &[]).await;
        planning::configure(&mut fixture, WorkMode::Plan).await;
        let turn = send(&fixture, "Give this conversation a short title").await;
        policy(&server.requests.lock().unwrap()[0], None);
        assert_eq!(read(&fixture.client, fixture.session.id).await, TITLE);
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert!(page.approvals.is_empty());
        assert!(
            tool_details(&fixture.client, &page)
                .await
                .iter()
                .any(|(name, detail)| {
                    let result = detail.result.as_ref().unwrap();
                    name == "set_session_title" && result == &json!({"title": TITLE})
                })
        );
        send(&fixture, "Continue the task").await;
        policy(server.requests.lock().unwrap().last().unwrap(), Some(TITLE));
        assert_eq!(read(&fixture.client, fixture.session.id).await, TITLE);
        let Output::Session(fork) = fixture
            .client
            .execute(fixture.client.prepare(Command::ForkConversation {
                session: fixture.session.id,
                through: turn,
                expected_revision: fixture.session.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("fork expected");
        };
        assert_eq!(read(&fixture.client, fork.id).await, TITLE);
        fixture.node.shutdown().await.unwrap();
        let node = Node::start_empty(fixture.directory.path().join("node"))
            .await
            .unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(read(&client, fixture.session.id).await, TITLE);
        assert_eq!(read(&client, fork.id).await, TITLE);
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn manual_name() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let mut fixture = Fixture::packages(remote, &server, &[]).await;
        let rename = |revision, title: &str| Command::RenameSession {
            session: fixture.session.id,
            expected_revision: revision,
            title: title.into(),
        };
        for title in [" ", "Invalid\nname"] {
            assert_eq!(
                fixture
                    .client
                    .execute(
                        fixture
                            .client
                            .prepare(rename(fixture.session.revision, title))
                    )
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        let request = fixture
            .client
            .prepare(rename(fixture.session.revision, "  Manual 中文 🙂  "));
        let Output::Session(renamed) = fixture.client.execute(request.clone()).await.unwrap()
        else {
            panic!("session expected");
        };
        assert_eq!(renamed.activity.title, "Manual 中文 🙂");
        assert_eq!(
            read(&fixture.client, renamed.id).await,
            renamed.activity.title
        );
        let Output::Session(replayed) = fixture.client.execute(request).await.unwrap() else {
            panic!("session expected");
        };
        assert_eq!(replayed.revision, renamed.revision);
        assert_eq!(
            fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(rename(fixture.session.revision, "Stale"))
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        fixture.session = renamed;
        let Output::Session(empty_fork) = fixture
            .client
            .execute(fixture.client.prepare(Command::ForkConversationAt {
                session: fixture.session.id,
                worktree: fixture.session.worktree,
                expected_revision: fixture.session.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected");
        };
        assert_eq!(read(&fixture.client, empty_fork.id).await, "Manual 中文 🙂");
        let turn = send(&fixture, "Continue the task").await;
        policy(
            server.requests.lock().unwrap().last().unwrap(),
            Some("Manual 中文 🙂"),
        );
        assert_eq!(
            read(&fixture.client, fixture.session.id).await,
            "Manual 中文 🙂"
        );
        let Output::Session(fork) = fixture
            .client
            .execute(fixture.client.prepare(Command::ForkConversation {
                session: fixture.session.id,
                through: turn,
                expected_revision: fixture.session.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected");
        };
        assert_eq!(read(&fixture.client, fork.id).await, "Manual 中文 🙂");
        fixture
            .client
            .execute(fixture.client.prepare(Command::RewindConversation {
                session: fixture.session.id,
                through: None,
                expected_head: turn,
                expected_revision: 1,
            }))
            .await
            .unwrap();
        assert_eq!(
            read(&fixture.client, fixture.session.id).await,
            "Manual 中文 🙂"
        );
        fixture.node.shutdown().await.unwrap();
        let node = Node::start_empty(fixture.directory.path().join("node"))
            .await
            .unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(read(&client, fixture.session.id).await, "Manual 中文 🙂");
        assert_eq!(read(&client, fork.id).await, "Manual 中文 🙂");
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
