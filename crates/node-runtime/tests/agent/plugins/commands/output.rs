use super::*;
use sailry_protocol::process::OutputStream;
use sailry_protocol::tool::OutputReference;

async fn projected(server: &Server) -> Value {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let value = server
                .requests
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find_map(|request| {
                    request["messages"].as_array()?.iter().find_map(|message| {
                        (message["role"] == "tool")
                            .then(|| message["content"].as_str())
                            .flatten()
                            .and_then(|text| serde_json::from_str::<Value>(text).ok())
                            .filter(|value| value["output"]["call"].is_string())
                    })
                });
            if let Some(value) = value {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("model output projection deadline")
}

#[tokio::test]
async fn retains_foreground_streams_and_canonical_results() {
    for remote in [false, true] {
        let gate = Arc::new(tokio::sync::Notify::new());
        let server = Server::held_after_tool(vec![(name("run_command"),json!({
            "command":"printf '\\033[31mhead\\033[0m\\n'; i=0; while [ \"$i\" -lt 4000 ]; do printf 'preserved Unicode 中文🙂 output row\\n'; i=$((i+1)); done; printf 'last failure\\n'; exit 7"
        }))],gate.clone()).await;
        let mut fixture = Fixture::packages(remote, &server, &["commands"]).await;
        planning::configure(&mut fixture, WorkMode::Code).await;
        let turn = approvals::submit(&fixture.client, fixture.session.id).await;
        let projected = projected(&server).await;
        assert!(
            projected["data"]["stdout"]["text"]
                .as_str()
                .unwrap()
                .ends_with("last failure\n")
        );
        assert!(
            !projected["data"]["stdout"]["text"]
                .as_str()
                .unwrap()
                .contains('\u{1b}')
        );
        let reference: OutputReference =
            serde_json::from_value(projected["output"].clone()).unwrap();
        let mut text = String::new();
        let mut offset = 0;
        loop {
            let Output::ToolOutput(page) = execute(
                &fixture,
                Command::ReadToolOutput {
                    turn,
                    output: reference.clone(),
                    stream: None,
                    offset,
                    limit: 997,
                },
            )
            .await
            else {
                panic!("original tool result page expected")
            };
            text.push_str(&page.text);
            let Some(next) = page.next_offset else {
                break;
            };
            offset = next;
        }
        let original: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            original["data"]["outcome"],
            json!({"kind":"exited","data":7})
        );
        assert!(
            original["data"]["stdout"]["text"]
                .as_str()
                .unwrap()
                .contains('\u{1b}')
        );
        let id = serde_json::from_value(original["sailry_command"]["id"].clone()).unwrap();
        assert!(
            matches!(execute(&fixture,Command::ListCommands {session:fixture.session.id}).await,Output::Commands(commands) if commands.is_empty())
        );
        let mut complete = String::new();
        let mut offset = 0;
        loop {
            let Output::CommandOutputPage(page) = execute(
                &fixture,
                Command::ReadToolOutput {
                    turn,
                    output: reference.clone(),
                    stream: Some(OutputStream::Stdout),
                    offset,
                    limit: 1013,
                },
            )
            .await
            else {
                panic!("retained stream expected")
            };
            assert!(!page.truncated && !page.invalid_utf8);
            complete.push_str(&page.text);
            let Some(next) = page.next_offset else {
                break;
            };
            offset = next;
        }
        assert_eq!(complete.lines().count(), 4002);
        assert!(complete.starts_with("\u{1b}[31mhead\u{1b}[0m\n"));
        assert!(complete.ends_with("last failure\n"));
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ReadCommandPage {
                    session: SessionId::new(),
                    id,
                    stream: OutputStream::Stdout,
                    offset: 0,
                    limit: 100,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let mut stale = reference.clone();
        stale.revision = "0".repeat(64);
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ReadToolOutput {
                    turn,
                    output: stale,
                    stream: None,
                    offset: 0,
                    limit: 100,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let profile = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        let node = Node::start_empty(profile).await.unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let Output::Session(session) = client
            .execute(client.prepare(Command::ReadSession {
                session: fixture.session.id,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let Output::QueuedTurn(resumed) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Inspect the earlier output".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("new invocation expected")
        };
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let page = history(&client, session.id).await;
                if page
                    .runs
                    .iter()
                    .any(|run| run.turn == resumed.id && run.status == Status::Running)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let Output::ToolOutput(saved) = client
            .execute(client.prepare(Command::ReadToolOutput {
                turn: resumed.id,
                output: reference,
                stream: None,
                offset: 0,
                limit: 100,
            }))
            .await
            .unwrap()
        else {
            panic!("persisted canonical output expected")
        };
        assert!(text.starts_with(&saved.text));
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadCommandPage {
                    session: session.id,
                    id,
                    stream: OutputStream::Stdout,
                    offset: 0,
                    limit: 100,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        gate.notify_waiters();
        node.shutdown().await.unwrap();
    }
}
