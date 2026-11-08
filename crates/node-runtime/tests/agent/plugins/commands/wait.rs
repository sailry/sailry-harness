use super::*;
use sailry_protocol::process::{OutputStream, Snapshot, Status as CommandStatus};

async fn start(server: &Server) -> (Fixture, RequestId) {
    let mut fixture = Fixture::packages(server, &["commands"]).await;
    planning::configure(&mut fixture, WorkMode::Code).await;
    let turn = approvals::rules::submit(&fixture.client, &fixture.session)
        .await
        .id;
    let page = finished(&fixture.client, fixture.session.id, turn).await;
    assert_eq!(page.runs.last().unwrap().status, Status::Completed);
    let result = results(&fixture.client, &page).await;
    let id = serde_json::from_value(result[0]["data"]["id"].clone()).unwrap();
    (fixture, id)
}

async fn read(fixture: &Fixture, id: RequestId, until: &str) -> Snapshot {
    let Output::CommandOutput(snapshot) = execute(
        fixture,
        Command::ReadCommand {
            session: fixture.session.id,
            id,
            wait_ms: 30_000,
            until: Some(until.into()),
        },
    )
    .await
    else {
        panic!("command output expected")
    };
    snapshot
}

async fn write(fixture: &Fixture, id: RequestId, input: &str) {
    assert_eq!(
        execute(
            fixture,
            Command::WriteCommand {
                session: fixture.session.id,
                id,
                input: input.into(),
            },
        )
        .await,
        Output::CommandInputAccepted {
            id,
            bytes: input.len(),
        }
    );
}

#[tokio::test]
async fn observes_output_substrings_and_exit() {
    for pty in [false, true] {
        let command = format!(
            "{}printf 'ready\\n'; while IFS= read -r value; do if [ \"$value\" = exit ]; then exit 7; fi; printf '%s' \"$value\"; done",
            if pty { "stty -echo; " } else { "" },
        );
        let server = Server::scripted_tools(vec![(
            name("run_command"),
            json!({"access":"unrestricted","command":command,"background":true,"pty":pty}),
        )])
        .await;
        let (fixture, id) = start(&server).await;
        let session = fixture.session.id;
        assert!(
            read(&fixture, id, "ready")
                .await
                .stdout
                .text
                .contains("ready")
        );
        let request = fixture.client.prepare(Command::ReadCommand {
            session,
            id,
            wait_ms: 30_000,
            until: Some("complete".into()),
        });
        let mut pending = tokio::time::timeout(
            Duration::from_secs(3),
            fixture.client.dispatch(request.clone()),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(pending.receipt.id, request.id);
        assert!(!pending.receipt.durable);
        write(&fixture, id, "com\n").await;
        read(&fixture, id, "com").await;
        assert!(matches!(
            pending.completion.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        write(&fixture, id, "plete\n").await;
        let Output::CommandOutput(snapshot) =
            tokio::time::timeout(Duration::from_secs(3), pending.completion)
                .await
                .unwrap()
                .unwrap()
                .unwrap()
        else {
            panic!("command output expected")
        };
        assert!(snapshot.stdout.text.ends_with("complete"));
        assert_eq!(snapshot.info.status, CommandStatus::Running);
        let pending = fixture
            .client
            .dispatch(fixture.client.prepare(Command::ReadCommandPage {
                session,
                id,
                stream: OutputStream::Stdout,
                offset: snapshot.stdout.text.len(),
                limit: 128,
                wait_ms: 30_000,
                until: Some(String::new()),
            }))
            .await
            .unwrap();
        write(&fixture, id, "next\n").await;
        let Output::CommandOutputPage(page) =
            tokio::time::timeout(Duration::from_secs(3), pending.completion)
                .await
                .unwrap()
                .unwrap()
                .unwrap()
        else {
            panic!("command page expected")
        };
        assert_eq!(page.text, "next");
        let pending = fixture
            .client
            .dispatch(fixture.client.prepare(Command::ReadCommand {
                session,
                id,
                wait_ms: 30_000,
                until: Some("absent".into()),
            }))
            .await
            .unwrap();
        write(&fixture, id, "exit\n").await;
        let Output::CommandOutput(snapshot) =
            tokio::time::timeout(Duration::from_secs(3), pending.completion)
                .await
                .unwrap()
                .unwrap()
                .unwrap()
        else {
            panic!("command output expected")
        };
        assert_eq!(
            snapshot.info.status,
            CommandStatus::Finished(Outcome::Exited(7))
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn stop_preserves_signal_cleanup() {
    let server = Server::scripted_tools(vec![(
        name("run_command"),
        json!({
            "access":"unrestricted",
            "command":"trap 'printf cleaned > cleanup.txt; exit 0' TERM; sleep 60 & child_pid=$!; printf '%s' \"$child_pid\" > child.pid; printf ready; wait \"$child_pid\"",
            "background":true,
        }),
    )])
    .await;
    let (fixture, id) = start(&server).await;
    read(&fixture, id, "ready").await;
    execute(
        &fixture,
        Command::StopCommand {
            session: fixture.session.id,
            id,
        },
    )
    .await;
    let snapshot = read(&fixture, id, "absent").await;
    assert_eq!(
        snapshot.info.status,
        CommandStatus::Finished(Outcome::Cancelled)
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("cleanup.txt")).unwrap(),
        "cleaned"
    );
    fixture.node.shutdown().await.unwrap();
    fixture.controller.close().await.unwrap();
}
