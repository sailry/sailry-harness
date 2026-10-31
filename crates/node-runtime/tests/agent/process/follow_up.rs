use super::*;
use sailry_protocol::conversation::background::{Kind, State};

#[tokio::test]
async fn keeps_quiet_completion_for_manual_reads() {
    for remote in [false, true] {
        let release = sailry_link::CancellationToken::new();
        let server = Server::markdown_held("Parent finished".into(), release.clone()).await;
        let fixture = Fixture::packages(remote, &server, &[]).await;
        let Output::QueuedTurn(parent) = fixture
            .client
            .execute(fixture.client.prepare(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Keep the existing task active".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        server.wait_count(1).await;
        let command = fixture.client.prepare(Command::RunCommand {
            turn: parent.id,
            command: "printf 'Quiet background evidence'".into(),
            description: Some("Background fixture".into()),
            access: sailry_protocol::process::Access::Workspace,
            cwd: String::new(),
            timeout_ms: 10_000,
            background: true,
            follow_up: false,
            pty: false,
            attachments: Vec::new(),
        });
        let Output::CommandStarted(info) = fixture.client.execute(command.clone()).await.unwrap()
        else {
            panic!("started command expected")
        };
        let id = format!("command:{}", info.id);
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let page = history(&fixture.client, fixture.session.id).await;
                if page
                    .background
                    .iter()
                    .any(|task| task.id == id && task.state == State::Pending)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("quiet completion must retain its evidence");
        release.cancel();
        let page = finished(&fixture.client, fixture.session.id, parent.id).await;
        assert!(page.runs.iter().all(|run| !run.notice));
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        let task = page.background.iter().find(|task| task.id == id).unwrap();
        assert_eq!(task.state, State::Pending);
        assert_eq!(task.kind, Kind::Command { id: info.id });
        assert_eq!(task.delivery, None);
        let Output::CommandOutput(snapshot) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadCommand {
                session: fixture.session.id,
                id: info.id,
            }))
            .await
            .unwrap()
        else {
            panic!("command snapshot expected")
        };
        assert_eq!(snapshot.stdout.text, "Quiet background evidence");
        assert_eq!(
            snapshot.info.status,
            sailry_protocol::process::Status::Finished(Outcome::Exited(0))
        );
        let mut changed = command.clone();
        let Command::RunCommand { follow_up, .. } = &mut changed.command else {
            unreachable!()
        };
        *follow_up = true;
        assert_eq!(
            fixture.client.execute(changed).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        assert_eq!(
            fixture.client.execute(command).await.unwrap(),
            Output::CommandStarted(info)
        );
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
