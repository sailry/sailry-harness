use super::*;
use sailry_link::CancellationToken;

async fn guide_parent(fixture: &Fixture, active: &QueuedTurn, text: &str) -> QueuedTurn {
    let admission = fixture
        .client
        .dispatch(fixture.client.prepare(Command::GuideTurn {
            recipient: active.id,
            expected_revision: active.revision,
            message: text.into(),
            immediate: false,
        }))
        .await
        .unwrap();
    assert!(admission.receipt.durable);
    let Output::QueuedTurn(turn) = admission.completion.await.unwrap().unwrap() else {
        panic!("guidance turn expected")
    };
    turn
}

fn user_text(request: &Value) -> String {
    crate::agent_support::task_request(request)["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "user")
        .map(|message| message_text(&message["content"]))
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_live(page: &Page, parent: TurnId, child: &Run) {
    assert_eq!(status(page, parent), Status::Running);
    let observed = page
        .children
        .iter()
        .find(|item| item.run.turn == child.turn)
        .unwrap();
    assert_eq!(observed.run.session, child.session);
    assert_eq!(observed.run.status, Status::Running);
}

fn question() -> (String, Value) {
    (
        "ask_user".into(),
        json!({"prompt":"Hold the parent boundary","input":{"kind":"text","multiline":true,"max_bytes":1024}}),
    )
}

async fn answer(fixture: &Fixture, question: QuestionId) {
    fixture
        .client
        .execute(fixture.client.prepare(Command::ResolveQuestion {
            session: fixture.session.id,
            question,
            response: question::Response::Answer(question::Answer::Text(
                "The parent tool is settled".into(),
            )),
        }))
        .await
        .unwrap();
}

#[tokio::test]
async fn wakes_foreground_and_batches_the_next_boundary() {
    for remote in [false, true] {
        let release = CancellationToken::new();
        let parent = Server::tools(vec![
            (
                "spawn_agent".into(),
                json!({"role":"review","task":"Hold the foreground inspection"}),
            ),
            question(),
        ])
        .await;
        let child = Server::markdown_held(
            "Foreground result survives steering".into(),
            release.clone(),
        )
        .await;
        let fixture = Fixture::start(remote, parent, child).await;
        let active = fixture.push("Delegate the foreground inspection").await;
        fixture.child.wait_count(1).await;
        let page = history(&fixture.client, fixture.session.id).await;
        assert_eq!(page.children.len(), 1);
        let child = page.children[0].run.clone();
        assert_live(&page, active.id, &child);
        assert_eq!(fixture.parent.requests.lock().unwrap().len(), 1);
        let wake = guide_parent(
            &fixture,
            &active,
            "Continue the parent while its child runs",
        )
        .await;
        let (page, question) = crate::questions::pending(&fixture.client, fixture.session.id).await;
        assert_live(&page, active.id, &child);
        assert_eq!(status(&page, wake.id), Status::Continued);
        let requests = fixture.parent.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert!(user_text(&requests[1]).contains("Continue the parent while its child runs"));
        assert!(
            !requests[1]["messages"]
                .to_string()
                .contains("Foreground result survives steering")
        );
        let texts = [
            "Steering boundary item one",
            "Steering boundary item two",
            "Steering boundary item three",
        ];
        let mut queued = Vec::new();
        for text in texts {
            queued.push(guide_parent(&fixture, &active, text).await);
        }
        assert_eq!(fixture.parent.requests.lock().unwrap().len(), 2);
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        let page = history(&fixture.client, fixture.session.id).await;
        assert_live(&page, active.id, &child);
        for input in &queued {
            assert_eq!(status(&page, input.id), Status::Queued);
        }
        answer(&fixture, question.id).await;
        fixture.parent.wait_count(3).await;
        let requests = fixture.parent.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 3);
        let delivered = user_text(&requests[2]);
        let mut previous = 0;
        for text in texts {
            assert_eq!(delivered.matches(text).count(), 1);
            let position = delivered.find(text).unwrap();
            assert!(position >= previous);
            previous = position + text.len();
        }
        let page = history(&fixture.client, fixture.session.id).await;
        assert_live(&page, active.id, &child);
        assert_eq!(
            status(&history(&fixture.client, child.session).await, child.turn),
            Status::Running
        );
        for input in &queued {
            let run = page.runs.iter().find(|run| run.turn == input.id).unwrap();
            assert_eq!(run.status, Status::Continued);
            assert_eq!(run.joined, Some(active.id));
        }
        assert!(!release.is_cancelled());
        release.cancel();
        fixture.parent.wait_count(4).await;
        let page = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(status(&page, active.id), Status::Completed);
        let child_page = finished(&fixture.client, child.session, child.turn).await;
        assert_eq!(status(&child_page, child.turn), Status::Completed);
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        received(
            &fixture.parent,
            child.session,
            "completed: Foreground result survives steering",
            1,
        );
        assert!(
            page.entries
                .iter()
                .filter(|entry| entry.author == "assistant")
                .all(|entry| entry.turn == active.id)
        );
        assert!(page.queue.items.is_empty());
        for (input, text) in queued.iter().zip(texts) {
            assert_eq!(
                page.entries
                    .iter()
                    .filter(|entry| entry.turn == input.id
                        && entry.author == "user"
                        && entry
                            .parts
                            .iter()
                            .any(|part| matches!(part, Part::Text(value) if value == text)))
                    .count(),
                1
            );
        }
        fixture.close().await;
    }
}

#[cfg(unix)]
async fn marker(files: &mut dyn sailry_link::Subscription, path: &std::path::Path) {
    tokio::time::timeout(COMPLETION_BUDGET, async {
        loop {
            if std::fs::read(path).is_ok_and(|bytes| bytes == b"x") {
                return;
            }
            files.next().await.unwrap();
        }
    })
    .await
    .expect("held command marker deadline");
}

/// Foreground commands are absent from the background inventory. The held
/// shell is the gate's only reader, so a non-blocking writer observes its exit.
#[cfg(unix)]
async fn terminated(gate: &std::path::Path) {
    use std::os::unix::fs::OpenOptionsExt;
    tokio::time::timeout(COMPLETION_BUDGET, async {
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(gate)
            {
                Err(error) if error.raw_os_error() == Some(libc::ENXIO) => return,
                Err(error) => panic!("command gate unavailable: {error}"),
                Ok(_) => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        }
    })
    .await
    .expect("immediate command cancellation deadline");
}

#[cfg(unix)]
#[tokio::test]
async fn immediate_interrupt_keeps_parent_and_child() {
    for remote in [false, true] {
        let release = CancellationToken::new();
        let command = "mkfifo steering.gate && exec 3<> steering.gate && printf x >> steering.marker && IFS= read -r signal <&3 && printf 'Command released'";
        let parent = Server::tools(vec![
            (
                "spawn_agent".into(),
                json!({"role":"review","task":"Keep the foreground child alive"}),
            ),
            (
                plugin_tool("commands", "run_command"),
                json!({"access":"unrestricted","command":command,"timeout_ms":60000}),
            ),
            question(),
        ])
        .await;
        let child = Server::markdown_held(
            "Child result after immediate steering".into(),
            release.clone(),
        )
        .await;
        let mut fixture = Fixture::start(remote, parent, child).await;
        let mut config = fixture.session.config.clone();
        config.permission = Permission::Full;
        let Output::Session(session) = fixture
            .client
            .execute(fixture.client.prepare(Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                config,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let transport = if remote {
            fixture
                .controller
                .handle()
                .remote(fixture.node.link().address())
        } else {
            fixture.node.local()
        };
        let mut files = transport
            .subscribe(Topic::Files(fixture.session.worktree))
            .await
            .unwrap();
        files.next().await.unwrap();
        let active = fixture
            .push("Keep the child while interrupting the parent tool")
            .await;
        fixture.child.wait_count(1).await;
        let child = history(&fixture.client, fixture.session.id).await.children[0]
            .run
            .clone();
        let wake = guide_parent(
            &fixture,
            &active,
            "Start the parent tool without ending its child",
        )
        .await;
        marker(files.as_mut(), &fixture.root().join("steering.marker")).await;
        let Output::QueuedTurn(selected) = fixture
            .client
            .execute(fixture.client.prepare(Command::QueueTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Interrupt the held command and continue this parent".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("queued turn expected")
        };
        let request = fixture.client.prepare(Command::GuideQueuedTurn {
            turn: selected.id,
            recipient: active.id,
            expected_revision: 1,
            immediate: true,
        });
        let admission = fixture.client.dispatch(request).await.unwrap();
        assert!(admission.receipt.durable);
        admission.completion.await.unwrap().unwrap();
        let (page, question) = crate::questions::pending(&fixture.client, fixture.session.id).await;
        assert_live(&page, active.id, &child);
        assert_eq!(status(&page, wake.id), Status::Continued);
        assert_eq!(status(&page, selected.id), Status::Continued);
        assert_eq!(
            status(&history(&fixture.client, child.session).await, child.turn),
            Status::Running
        );
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        let requests = fixture.parent.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 3);
        assert!(
            user_text(&requests[2]).contains("Interrupt the held command and continue this parent")
        );
        assert!(
            requests[2]["messages"]
                .to_string()
                .contains("outcome may be unknown")
        );
        assert!(
            requests[2]["messages"]
                .to_string()
                .contains("do not replay")
        );
        let cancelled: Vec<_> = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| match part {
                Part::ToolResult { summary, .. } => {
                    summary.failure.as_ref().and_then(|failure| failure.code)
                }
                _ => None,
            })
            .collect();
        assert_eq!(cancelled, [ErrorCode::Cancelled]);
        assert_eq!(
            std::fs::read(fixture.root().join("steering.marker")).unwrap(),
            b"x"
        );
        terminated(&fixture.root().join("steering.gate")).await;
        answer(&fixture, question.id).await;
        release.cancel();
        let page = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(status(&page, active.id), Status::Completed);
        assert_eq!(
            status(
                &finished(&fixture.client, child.session, child.turn).await,
                child.turn
            ),
            Status::Completed
        );
        received(
            &fixture.parent,
            child.session,
            "completed: Child result after immediate steering",
            1,
        );
        assert_eq!(
            std::fs::read(fixture.root().join("steering.marker")).unwrap(),
            b"x"
        );
        let calls = tool_calls(&fixture.client, &page).await;
        assert_eq!(
            calls
                .iter()
                .filter(|(_, detail)| detail.arguments["command"] == command)
                .count(),
            1
        );
        let results = tool_details(&fixture.client, &page).await;
        let commands: Vec<_> = results
            .iter()
            .filter(|(_, detail)| detail.arguments["command"] == command)
            .collect();
        assert_eq!(commands.len(), 1);
        assert!(
            commands[0]
                .1
                .result
                .as_ref()
                .unwrap()
                .to_string()
                .contains("outcome may be unknown")
        );
        assert!(
            page.entries
                .iter()
                .filter(|entry| entry.author == "assistant")
                .all(|entry| entry.turn == active.id)
        );
        assert!(page.queue.items.is_empty());
        drop(files);
        fixture.close().await;
    }
}
