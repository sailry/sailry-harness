use super::fixture::Fixture;
use super::*;

fn status(page: &Page, turn: TurnId) -> Status {
    page.runs
        .iter()
        .find(|run| run.turn == turn)
        .unwrap()
        .status
}

async fn held(remote: bool) -> Fixture {
    let parent = Server::delegation(Some("review")).await;
    let child = Server::batch(vec![
        (
            plugin_tool("files", "write_file"),
            json!({"path":"first.txt","text":"settled evidence","expected_revision":null}),
        ),
        (
            plugin_tool("files", "write_file"),
            json!({"path":"second.txt","text":"denied evidence","expected_revision":null}),
        ),
    ])
    .await;
    Fixture::start(remote, parent, child).await
}

async fn direction(fixture: &Fixture, active: &QueuedTurn) -> QueuedTurn {
    let Output::QueuedTurn(queued) = fixture
        .client
        .execute(fixture.client.prepare(Command::QueueTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Main correction: guide the running child and collect its answer".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("queued turn expected")
    };
    let request = fixture.client.prepare(Command::GuideQueuedTurn {
        turn: queued.id,
        recipient: active.id,
        expected_revision: 1,
    });
    let admission = fixture.client.dispatch(request).await.unwrap();
    assert!(admission.receipt.durable);
    admission.completion.await.unwrap().unwrap();
    queued
}

async fn guided(fixture: &Fixture, child: SessionId) -> (Page, QueuedMessage) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let page = history(&fixture.client, child).await;
            if let Some(item) = page.queue.items.first() {
                let Output::QueuedMessage(message) = fixture
                    .client
                    .execute(
                        fixture
                            .client
                            .prepare(Command::ReadQueuedTurn { turn: item.turn }),
                    )
                    .await
                    .unwrap()
                else {
                    panic!("queued message expected")
                };
                return (page, message);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("internal child guidance deadline")
}

#[tokio::test]
async fn main_direction_reaches_running_child() {
    for remote in [false, true] {
        let mut fixture = held(remote).await;
        let active = fixture.push("Parent-only private context").await;
        let child = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        let (_, first) = crate::approvals::pending(&fixture.client, child.id).await;
        fixture.parent.wait_count(2).await;
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        let origin = child.delegation.clone().unwrap();
        let directed = direction(&fixture, &active).await;
        let (page, child_direction) = guided(&fixture, child.id).await;
        assert_eq!(status(&page, first.turn), Status::Running);
        assert_eq!(status(&page, child_direction.turn.id), Status::Queued);
        assert_eq!(child_direction.turn.config, child.config);
        assert_eq!(child_direction.turn.roles, child.roles);
        assert_eq!(
            child_direction.message.text,
            "Apply the child correction after all writes settle"
        );
        assert_eq!(child.config.permission, sailry_protocol::Permission::Ask);
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        let parent = history(&fixture.client, fixture.session.id).await;
        assert_eq!(status(&parent, active.id), Status::Continued);
        assert_eq!(status(&parent, directed.id), Status::Running);
        assert!(
            results(&parent)
                .iter()
                .any(|result| result["recipient"] == json!(first.turn)
                    && result["turn"] == json!(child_direction.turn.id))
        );
        // Provider/catalog changes do not replace either admitted child's capture.
        fixture.provider.endpoint = "http://127.0.0.1:1/v1".into();
        fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                provider: fixture.provider.clone(),
                expected_revision: fixture.provider.revision,
            }))
            .await
            .unwrap();
        let mut role = fixture.role.clone();
        role.instructions = "Changed catalog instructions".into();
        fixture
            .client
            .execute(fixture.client.prepare(Command::PutRole {
                role,
                expected_revision: fixture.role.revision,
            }))
            .await
            .unwrap();
        fixture
            .client
            .execute(fixture.client.prepare(Command::ResolveApproval {
                session: child.id,
                approval: first.id,
                decision: Decision::Approve,
            }))
            .await
            .unwrap();
        let (page, second) = crate::approvals::pending(&fixture.client, child.id).await;
        assert_ne!(first.id, second.id);
        assert_eq!(status(&page, first.turn), Status::Running);
        assert_eq!(status(&page, child_direction.turn.id), Status::Queued);
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        fixture
            .client
            .execute(fixture.client.prepare(Command::ResolveApproval {
                session: child.id,
                approval: second.id,
                decision: Decision::Deny,
            }))
            .await
            .unwrap();
        let child_page = finished(&fixture.client, child.id, child_direction.turn.id).await;
        let parent = finished(&fixture.client, fixture.session.id, directed.id).await;
        assert_eq!(status(&child_page, first.turn), Status::Continued);
        assert_eq!(
            status(&child_page, child_direction.turn.id),
            Status::Completed
        );
        assert_eq!(status(&parent, directed.id), Status::Completed);
        let returned = results(&parent);
        let final_child = returned
            .iter()
            .find(|result| result["status"] == "completed" && result["session"] == json!(child.id))
            .unwrap();
        assert_eq!(final_child["turn"], json!(child_direction.turn.id));
        assert_eq!(final_child["response"], "answer-fixture-child");
        assert!(
            returned
                .iter()
                .any(|result| result["guidance_pending"] == true)
        );
        assert!(
            parent
                .children
                .iter()
                .all(|summary| summary.origin == *origin)
        );
        let requests = fixture.child.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        let context = requests[1]["messages"].to_string();
        assert!(context.contains("settled evidence"));
        assert!(context.contains("Apply the child correction"));
        assert!(context.contains("Frozen review instructions"));
        assert!(!context.contains("Changed catalog instructions"));
        assert!(!context.contains("Parent-only private context"));
        assert!(
            requests
                .iter()
                .all(
                    |request| request["tools"].as_array().unwrap().iter().all(|tool| {
                        !["spawn_agent", "guide_agent", "wait_agent"]
                            .iter()
                            .any(|name| tool["function"]["name"] == plugin_tool("delegation", name))
                    })
                )
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root().join("first.txt")).unwrap(),
            "settled evidence"
        );
        assert!(!fixture.root().join("second.txt").exists());
        let counts = (
            fixture.parent.requests.lock().unwrap().len(),
            requests.len(),
        );
        let fixture = fixture.restart().await;
        assert_eq!(
            history(&fixture.client, child.id).await.entries,
            child_page.entries
        );
        assert_eq!(
            history(&fixture.client, fixture.session.id).await.children,
            parent.children
        );
        assert_eq!(
            (
                fixture.parent.requests.lock().unwrap().len(),
                fixture.child.requests.lock().unwrap().len()
            ),
            counts
        );
        fixture.close().await;
    }
}

#[tokio::test]
async fn stop_preserves_child_direction_without_replay() {
    for remote in [false, true] {
        let fixture = held(remote).await;
        let active = fixture.push("Perform the delegated task").await;
        let child = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        let (_, approval) = crate::approvals::pending(&fixture.client, child.id).await;
        let directed = direction(&fixture, &active).await;
        let (_, queued) = guided(&fixture, child.id).await;
        fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::StopTurn { turn: directed.id }),
            )
            .await
            .unwrap();
        let parent = finished(&fixture.client, fixture.session.id, directed.id).await;
        assert_eq!(status(&parent, directed.id), Status::Cancelled);
        let child_page = finished(&fixture.client, child.id, approval.turn).await;
        assert_eq!(status(&child_page, approval.turn), Status::Cancelled);
        assert!(child_page.queue.paused);
        assert_eq!(child_page.queue.items.len(), 1);
        assert_eq!(child_page.queue.items[0].turn, queued.turn.id);
        assert_eq!(child_page.queue.items[0].guidance, None);
        assert!(
            child_page
                .approvals
                .iter()
                .any(|record| record.id == approval.id && record.state == ApprovalState::Cancelled)
        );
        assert!(
            child_page
                .approvals
                .iter()
                .all(|record| record.state != ApprovalState::Pending)
        );
        assert!(!fixture.root().join("first.txt").exists());
        let counts = (
            fixture.parent.requests.lock().unwrap().len(),
            fixture.child.requests.lock().unwrap().len(),
        );
        let fixture = fixture.restart().await;
        let restored = history(&fixture.client, child.id).await;
        assert_eq!(restored.entries, child_page.entries);
        assert_eq!(restored.queue, child_page.queue);
        let Output::QueuedMessage(retained) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadQueuedTurn {
                turn: queued.turn.id,
            }))
            .await
            .unwrap()
        else {
            panic!("queued input expected")
        };
        assert_eq!(retained.message, queued.message);
        assert_eq!(
            (
                fixture.parent.requests.lock().unwrap().len(),
                fixture.child.requests.lock().unwrap().len()
            ),
            counts
        );
        let inspection = fixture
            .push("Report the retained child direction without resuming it")
            .await;
        let parent = finished(&fixture.client, fixture.session.id, inspection.id).await;
        assert_eq!(status(&parent, inspection.id), Status::Completed);
        assert!(
            results(&parent)
                .iter()
                .any(|result| result["session"] == json!(child.id)
                    && result["turn"] == json!(queued.turn.id)
                    && result["paused"] == true)
        );
        assert_eq!(fixture.child.requests.lock().unwrap().len(), counts.1);
        assert_eq!(
            history(&fixture.client, child.id).await.queue,
            restored.queue
        );
        fixture.close().await;
    }
}

#[tokio::test]
async fn continues_after_background_answer() {
    for remote in [false, true] {
        let release = sailry_link::CancellationToken::new();
        let parent = Server::tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"role":"review","task":"Wait for the held response","background":true}),
        )])
        .await;
        let child = Server::markdown_held("Final child evidence".into(), release.clone()).await;
        let fixture = Fixture::start(remote, parent, child).await;
        let active = fixture.push("Perform the independent task").await;
        let child = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        fixture.child.wait_count(1).await;
        let answer = format!("answer-{}", fixture.session.config.model);
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let page = history(&fixture.client, fixture.session.id).await;
                if page.entries.iter().any(|entry| {
                    entry.turn == active.id
                        && entry.author != "user"
                        && entry.parts.contains(&Part::Text(answer.clone()))
                }) {
                    assert_eq!(status(&page, active.id), Status::Running);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("background answer deadline");
        let directed = direction(&fixture, &active).await;
        let parent = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(status(&parent, active.id), Status::Continued);
        assert_eq!(parent.children[0].run.status, Status::Running);
        assert!(!parent.queue.paused);
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        release.cancel();
        let parent = finished(&fixture.client, fixture.session.id, directed.id).await;
        assert_eq!(status(&parent, active.id), Status::Continued);
        assert_eq!(status(&parent, directed.id), Status::Completed);
        assert_eq!(parent.children[0].run.status, Status::Completed);
        let page = history(&fixture.client, child.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert!(page.entries.iter().any(|entry| {
            entry
                .parts
                .contains(&Part::Text("Final child evidence".into()))
        }));
        assert_eq!(fixture.parent.requests.lock().unwrap().len(), 3);
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        fixture.close().await;
    }
}

#[tokio::test]
async fn completion_waits_for_background_cleanup() {
    for remote in [false, true] {
        let release = sailry_link::CancellationToken::new();
        let parent = Server::turn_tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"role":"review","task":"Wait for the held response","background":true}),
        )])
        .await;
        let child = Server::markdown_held("Final child evidence".into(), release.clone()).await;
        let fixture = Fixture::start(remote, parent, child).await;
        let active = fixture.push("Perform the independent task").await;
        let child = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        fixture.parent.wait_count(2).await;
        fixture.child.wait_count(1).await;
        let page = history(&fixture.client, fixture.session.id).await;
        assert_eq!(status(&page, active.id), Status::Running);
        assert_eq!(page.children[0].run.status, Status::Running);
        release.cancel();
        let parent = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(status(&parent, active.id), Status::Completed);
        assert_eq!(parent.children[0].run.status, Status::Completed);
        let page = history(&fixture.client, child.id).await;
        assert!(page.entries.iter().any(|entry| {
            entry
                .parts
                .contains(&Part::Text("Final child evidence".into()))
        }));
        fixture.close().await;
    }
}

#[tokio::test]
async fn default_background_child_inherits_permissions() {
    for remote in [false, true] {
        let parent = Server::parallel(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"task":"Independent default child","background":true}),
        )])
        .await;
        let fixture = Fixture::start(remote, parent, Server::start(false).await).await;
        let active = fixture.push("Parent-only private context").await;
        let parent = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(status(&parent, active.id), Status::Completed);
        let child = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        assert_eq!(child.config, fixture.session.config);
        assert_eq!(child.config.permission, sailry_protocol::Permission::Ask);
        assert_eq!(child.delegation.as_ref().unwrap().role, None);
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 0);
        let page = history(&fixture.client, child.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert!(page.approvals.is_empty());
        assert!(!page.entries.iter().any(|entry| {
            entry
                .parts
                .contains(&Part::Text("Parent-only private context".into()))
        }));
        assert_eq!(results(&parent)[0]["session"], json!(child.id));
        fixture.close().await;
    }
}
