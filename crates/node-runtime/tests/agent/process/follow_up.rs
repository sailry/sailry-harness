use super::*;
use sailry_protocol::conversation::background::{Kind, State};

#[cfg(feature = "test-support")]
#[tokio::test]
async fn notifies_idle_session_of_stall() {
    let release = sailry_link::CancellationToken::new();
    let server = Server::markdown_held("Checked the listener".into(), release.clone()).await;
    let fixture = Fixture::packages(&server, &[]).await;
    fixture
        .node
        .set_command_stall_fixture(Duration::from_millis(300));
    let Output::QueuedTurn(parent) = fixture
        .client
        .execute(fixture.client.prepare(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Wait for the receipt".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    server.wait_count(1).await;
    let Output::CommandStarted(info) = fixture
        .client
        .execute(fixture.client.prepare(Command::RunCommand {
            turn: parent.id,
            command: "printf 'Waiting for receipt'; sleep 60".into(),
            description: Some("Receipt listener".into()),
            access: sailry_protocol::process::Access::Workspace,
            cwd: String::new(),
            timeout_ms: 10_000,
            background: true,
            follow_up: true,
            pty: false,
            attachments: Vec::new(),
        }))
        .await
        .unwrap()
    else {
        panic!("started command expected")
    };
    let id = format!("stall:{}", info.id);
    // A busy session keeps the stall until its current task finishes.
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let page = history(&fixture.client, fixture.session.id).await;
            if page.background.iter().any(|task| {
                task.id == id
                    && task.state == State::Pending
                    && task.summary.as_deref() == Some("Waiting for receipt")
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("silent command must record a stall");
    release.cancel();
    let page = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let page = history(&fixture.client, fixture.session.id).await;
            if page
                .runs
                .iter()
                .any(|run| run.notice && run.status == Status::Completed)
            {
                break page;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("idle session must receive the stall");
    let notice = page.runs.iter().find(|run| run.notice).unwrap();
    assert_eq!(notice.notices.len(), 1);
    assert_eq!(notice.notices[0].id, id);
    assert_eq!(
        notice.notices[0].outcome,
        sailry_protocol::conversation::background::Outcome::Stalled
    );
    assert_eq!(notice.notices[0].kind, Kind::Command { id: info.id });
    let stall = page.background.iter().find(|task| task.id == id).unwrap();
    assert_eq!(stall.state, State::Delivered);
    let command = format!("command:{}", info.id);
    let task = page
        .background
        .iter()
        .find(|task| task.id == command)
        .unwrap();
    assert_eq!(task.state, State::Running);
    let request =
        crate::agent_support::task_request(server.requests.lock().unwrap().last().unwrap());
    let content = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rfind(|message| message["role"] == "user")
        .unwrap()["content"]
        .as_str()
        .unwrap();
    let data: serde_json::Value =
        serde_json::from_str(content.split_once('\n').unwrap().1).unwrap();
    assert_eq!(data[0]["notice"]["outcome"], "stalled");
    assert_eq!(data[0]["output"]["stdout_tail"], "Waiting for receipt");
    // Detection never stops the command.
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
    assert_eq!(
        snapshot.info.status,
        sailry_protocol::process::Status::Running
    );
    assert_eq!(server.requests.lock().unwrap().len(), 2);
    fixture.controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn keeps_quiet_completion_for_manual_reads() {
    let release = sailry_link::CancellationToken::new();
    let server = Server::markdown_held("Parent finished".into(), release.clone()).await;
    let fixture = Fixture::packages(&server, &[]).await;
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

#[tokio::test]
async fn continues_consecutive_notice_turns() {
    let start = Arc::new(tokio::sync::Notify::new());
    let finish = Arc::new(tokio::sync::Notify::new());
    let server = Server::held(start.clone(), finish.clone()).await;
    let fixture = Fixture::packages(&server, &[]).await;
    let Output::QueuedTurn(parent) = fixture
        .client
        .execute(fixture.client.prepare(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Watch the background work".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    let mut owner = parent.id;
    for round in 1..=6 {
        // Commands start only in a running turn, which holds its model response.
        server.wait_count(round).await;
        let gate = fixture.root.join(format!("gate-{round}"));
        let Output::CommandStarted(info) = fixture
            .client
            .execute(fixture.client.prepare(Command::RunCommand {
                turn: owner,
                command: format!(
                    "while [ ! -e '{}' ]; do sleep 0.02; done; printf 'Round {round}'",
                    gate.display()
                ),
                description: Some("Background fixture".into()),
                access: sailry_protocol::process::Access::Workspace,
                cwd: String::new(),
                timeout_ms: 30_000,
                background: true,
                follow_up: true,
                pty: false,
                attachments: Vec::new(),
            }))
            .await
            .unwrap()
        else {
            panic!("started command expected")
        };
        start.notify_one();
        finish.notify_one();
        finished(&fixture.client, fixture.session.id, owner).await;
        std::fs::write(&gate, "").unwrap();
        server.wait_count(round + 1).await;
        let id = format!("command:{}", info.id);
        owner = history(&fixture.client, fixture.session.id)
            .await
            .runs
            .iter()
            .find(|run| run.notice && run.notices.iter().any(|notice| notice.id == id))
            .expect("an idle session continues with each completion")
            .turn;
    }
    start.notify_one();
    finish.notify_one();
    finished(&fixture.client, fixture.session.id, owner).await;
    let page = history(&fixture.client, fixture.session.id).await;
    let notices: Vec<_> = page.runs.iter().filter(|run| run.notice).collect();
    assert_eq!(notices.len(), 6);
    for notice in notices {
        assert_eq!(notice.status, Status::Completed);
        assert_eq!(notice.notices.len(), 1);
        let task = page
            .background
            .iter()
            .find(|task| task.id == notice.notices[0].id)
            .unwrap();
        assert_eq!(task.state, State::Delivered);
        assert_eq!(task.delivery, Some(notice.turn));
    }
    assert_eq!(server.requests.lock().unwrap().len(), 7);
    fixture.controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}
