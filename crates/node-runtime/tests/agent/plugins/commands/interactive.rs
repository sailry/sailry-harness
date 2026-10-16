use super::*;
use sailry_protocol::process::{Snapshot, Status as CommandStatus};

async fn output(fixture: &Fixture, id: RequestId) -> Snapshot {
    let Output::CommandOutput(output) = execute(
        fixture,
        Command::ReadCommand {
            session: fixture.session.id,
            id,
        },
    )
    .await
    else {
        panic!("command output expected");
    };
    output
}

#[tokio::test]
async fn writes_to_pipes_and_pty() {
    for remote in [false, true] {
        for pty in [false, true] {
            let command = format!(
                "{}printf 'ready>'; IFS= read -r value; printf '%s\\n' \"$value\" >> received.txt; printf 'again>'; IFS= read -r value; printf '%s\\n' \"$value\" >> received.txt; printf 'done'",
                if pty {
                    "test -t 0 && test -t 1 || exit 9; "
                } else {
                    "test ! -t 0 || exit 9; "
                }
            );
            let server = Server::scripted_tools(vec![(
                name("run_command"),
                json!({"command":command,"background":true,"pty":pty}),
            )])
            .await;
            let mut fixture = Fixture::packages(remote, &server, &["commands"]).await;
            let mut config = fixture.session.config.clone();
            config.permission = sailry_protocol::Permission::Full;
            let Output::Session(session) = execute(
                &fixture,
                Command::SetSessionConfig {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    config,
                },
            )
            .await
            else {
                panic!("session expected");
            };
            fixture.session = session;
            let turn = approvals::rules::submit(&fixture.client, &fixture.session)
                .await
                .id;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                page.runs
            );
            let result = results(&page);
            let id: RequestId = serde_json::from_value(result[0]["data"]["id"].clone()).unwrap();
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let snapshot = output(&fixture, id).await;
                    assert_eq!(snapshot.info.pty, pty);
                    assert!(matches!(snapshot.info.status, CommandStatus::Running));
                    if snapshot.stdout.text.contains("ready>") {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            for (command, expected) in [
                (
                    Command::WriteCommand {
                        session: SessionId::new(),
                        id,
                        input: "wrong\n".into(),
                    },
                    ErrorCode::NotFound,
                ),
                (
                    Command::WriteCommand {
                        session: fixture.session.id,
                        id,
                        input: "a".repeat(65537),
                    },
                    ErrorCode::InvalidRequest,
                ),
            ] {
                let error = fixture
                    .client
                    .execute(fixture.client.prepare(command))
                    .await
                    .unwrap_err();
                assert_eq!(error.code, expected);
            }
            server.replace_tools(vec![(
                name("write_command"),
                json!({"id":id,"input":"first 🙂\n"}),
            )]);
            let turn = approvals::rules::submit(&fixture.client, &fixture.session)
                .await
                .id;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                page.runs
            );
            assert_eq!(
                results(&page).last().unwrap()["kind"],
                "command_input_accepted"
            );
            tokio::time::timeout(Duration::from_secs(5), async {
                while !std::fs::read_to_string(fixture.root.join("received.txt"))
                    .is_ok_and(|text| text == "first 🙂\n")
                {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            let request = fixture.client.prepare(Command::WriteCommand {
                session: fixture.session.id,
                id,
                input: "second\n".into(),
            });
            let accepted = fixture.client.execute(request.clone()).await.unwrap();
            assert_eq!(accepted, Output::CommandInputAccepted { id, bytes: 7 });
            assert_eq!(
                fixture.client.execute(request.clone()).await.unwrap(),
                accepted
            );
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let snapshot = output(&fixture, id).await;
                    if let CommandStatus::Finished(outcome) = &snapshot.info.status {
                        assert_eq!(*outcome, Outcome::Exited(0), "{snapshot:?}");
                        assert!(snapshot.stdout.text.contains("done"));
                        assert!(!snapshot.stdout.invalid_utf8);
                        break;
                    }
                    assert!(
                        matches!(snapshot.info.status, CommandStatus::Running),
                        "{snapshot:?}"
                    );
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            assert_eq!(
                std::fs::read_to_string(fixture.root.join("received.txt")).unwrap(),
                "first 🙂\nsecond\n"
            );
            assert_eq!(fixture.client.execute(request).await.unwrap(), accepted);
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::WriteCommand {
                        session: fixture.session.id,
                        id,
                        input: "late\n".into()
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::Conflict
            );
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
