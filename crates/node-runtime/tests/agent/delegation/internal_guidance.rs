use super::fixture::Fixture;
use super::*;

mod hooks;

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
        assert_eq!(status(&parent, active.id), Status::Running);
        assert_eq!(status(&parent, directed.id), Status::Continued);
        assert!(
            results(&fixture.client, &parent)
                .await
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
        let child_page = finished(&fixture.client, child.id, first.turn).await;
        let parent = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(status(&child_page, first.turn), Status::Completed);
        assert_eq!(
            status(&child_page, child_direction.turn.id),
            Status::Continued
        );
        assert_eq!(status(&parent, active.id), Status::Completed);
        let returned = results(&fixture.client, &parent).await;
        let final_child = returned
            .iter()
            .find(|result| result["status"] == "completed" && result["session"] == json!(child.id))
            .unwrap();
        assert_eq!(final_child["turn"], json!(first.turn));
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
        direction(&fixture, &active).await;
        let (_, queued) = guided(&fixture, child.id).await;
        fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::StopTurn { turn: active.id }),
            )
            .await
            .unwrap();
        let parent = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(status(&parent, active.id), Status::Cancelled);
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
            results(&fixture.client, &parent)
                .await
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
        let title = "补充库存模块测试 🙂";
        let parent = Server::scripted_tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"role":"review","title":title,"task":"Wait for the held response","background":true}),
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
        let parent = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(status(&parent, active.id), Status::Completed);
        assert_eq!(parent.children[0].run.status, Status::Running);
        assert!(!parent.queue.paused);
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        assert_eq!(
            results(&fixture.client, &parent).await[0]["session"],
            json!(child.id)
        );
        fixture.parent.replace_tools(vec![(
            plugin_tool("delegation", "guide_agent"),
            json!({"session":child.id,"message":"Keep the final child evidence"}),
        )]);
        let directed = fixture.push("Guide the independent child").await;
        let parent = finished(&fixture.client, fixture.session.id, directed.id).await;
        assert_eq!(status(&parent, active.id), Status::Completed);
        assert_eq!(status(&parent, directed.id), Status::Completed);
        assert!(
            parent
                .children
                .iter()
                .any(|child| child.run.status == Status::Running)
        );
        let entry = parent
            .entries
            .iter()
            .find(|entry| {
                entry.turn == directed.id
                    && entry
                        .parts
                        .iter()
                        .any(|part| matches!(part, Part::ToolResult { .. }))
            })
            .unwrap();
        let index = entry
            .parts
            .iter()
            .position(|part| matches!(part, Part::ToolResult { .. }))
            .unwrap();
        let guidance = tool_detail(&fixture.client, &parent, entry, index)
            .await
            .result
            .unwrap();
        assert_eq!(guidance["session"], json!(child.id));
        assert!(guidance["recipient"].is_string());
        assert_eq!(guidance["status"], "queued");
        let observed = crate::progress::observe(&fixture.client, fixture.session.id).await;
        let page = &observed.snapshot.as_ref().unwrap().page;
        let canonical = page
            .children
            .iter()
            .find(|value| value.run.session == child.id)
            .unwrap();
        assert!(canonical.origin.role.is_some());
        let expected = format!("{} {title}", canonical.name.as_deref().unwrap());
        let guide = observed
            .calls
            .iter()
            .find(|call| {
                call.turn == directed.id && call.name == plugin_tool("delegation", "guide_agent")
            })
            .unwrap();
        let guide = guide.with_detail(
            crate::agent_support::details::read(
                &fixture.client,
                guide.detail_reference(page).unwrap(),
            )
            .await
            .unwrap(),
        );
        assert_eq!(guide.arguments(page).unwrap()["session"], json!(child.id));
        assert_eq!(
            guide
                .resolved
                .as_ref()
                .unwrap()
                .input
                .summary
                .as_ref()
                .unwrap()
                .text,
            expected
        );
        assert!(!expected.contains(&child.id.to_string()));
        fixture.parent.replace_tools(vec![(
            plugin_tool("delegation", "wait_agent"),
            json!({"session":child.id}),
        )]);
        let waiting = fixture.push("Collect the independent child answer").await;
        fixture.parent.wait_count(5).await;
        let parent = history(&fixture.client, fixture.session.id).await;
        assert_eq!(status(&parent, waiting.id), Status::Running);
        assert!(
            parent
                .children
                .iter()
                .any(|child| child.run.status == Status::Running)
        );
        assert!(
            !parent
                .entries
                .iter()
                .filter(|entry| entry.turn == waiting.id)
                .flat_map(|entry| &entry.parts)
                .any(|part| matches!(part, Part::ToolResult { .. }))
        );
        release.cancel();
        let parent = finished(&fixture.client, fixture.session.id, waiting.id).await;
        assert_eq!(status(&parent, active.id), Status::Completed);
        assert_eq!(status(&parent, directed.id), Status::Completed);
        assert_eq!(status(&parent, waiting.id), Status::Completed);
        assert!(parent.children.iter().all(|child| !matches!(
            child.run.status,
            Status::Queued | Status::Running | Status::Stopping
        )));
        let entry = parent
            .entries
            .iter()
            .find(|entry| {
                entry.turn == waiting.id
                    && entry
                        .parts
                        .iter()
                        .any(|part| matches!(part, Part::ToolResult { .. }))
            })
            .unwrap();
        let index = entry
            .parts
            .iter()
            .position(|part| matches!(part, Part::ToolResult { .. }))
            .unwrap();
        let waited = tool_detail(&fixture.client, &parent, entry, index)
            .await
            .result
            .unwrap();
        assert_eq!(waited["session"], json!(child.id));
        assert_eq!(waited["status"], "completed");
        assert_eq!(waited["response"], "Final child evidence");
        let page = history(&fixture.client, child.id).await;
        assert!(page.runs.iter().any(|run| run.status == Status::Completed));
        assert!(page.entries.iter().any(|entry| {
            entry
                .parts
                .contains(&Part::Text("Final child evidence".into()))
        }));
        assert_eq!(fixture.parent.requests.lock().unwrap().len(), 6);
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 2);
        fixture.close().await;
    }
}

#[tokio::test]
async fn completes_before_background_child() {
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
        fixture.parent.wait_count(2).await;
        fixture.child.wait_count(1).await;
        let parent = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(status(&parent, active.id), Status::Completed);
        assert_eq!(parent.children[0].run.status, Status::Running);
        assert_eq!(
            child.delegation.as_ref().unwrap().session,
            fixture.session.id
        );
        let request = fixture
            .client
            .prepare(Command::StopTurn { turn: active.id });
        let admission = fixture.client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let stopped = admission.completion.await.unwrap().unwrap();
        let Output::Run(run) = &stopped else {
            panic!("run expected")
        };
        assert_eq!(run.status, Status::Completed);
        assert_eq!(fixture.client.execute(request).await.unwrap(), stopped);
        let parent = history(&fixture.client, fixture.session.id).await;
        assert_eq!(status(&parent, active.id), Status::Completed);
        assert_eq!(parent.children[0].run.status, Status::Running);
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        release.cancel();
        let page = history(&fixture.client, child.id).await;
        let finished_child = finished(&fixture.client, child.id, page.runs[0].turn).await;
        assert_eq!(finished_child.runs[0].status, Status::Completed);
        let parent = history(&fixture.client, fixture.session.id).await;
        assert_eq!(status(&parent, active.id), Status::Completed);
        assert_eq!(parent.children[0].run.status, Status::Completed);
        let page = history(&fixture.client, child.id).await;
        assert!(page.entries.iter().any(|entry| {
            entry
                .parts
                .contains(&Part::Text("Final child evidence".into()))
        }));
        fixture.parent.wait_count(3).await;
        let parent = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let page = history(&fixture.client, fixture.session.id).await;
                if page
                    .runs
                    .iter()
                    .any(|run| run.notice && run.status == Status::Completed)
                    && page.background.iter().any(|task| {
                        task.state == sailry_protocol::conversation::background::State::Delivered
                    })
                {
                    break page;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("completion notice must reach canonical input and finish");
        let notice = parent.runs.iter().find(|run| run.notice).unwrap();
        assert!(notice.input.is_none());
        assert_eq!(notice.notices.len(), 1);
        let task = parent
            .background
            .iter()
            .find(|task| task.id == notice.notices[0].id)
            .unwrap();
        assert_eq!(task.session, fixture.session.id);
        assert_eq!(task.delivery, Some(notice.turn));
        assert_eq!(
            task.kind,
            sailry_protocol::conversation::background::Kind::Agent {
                session: child.id,
                turn: finished_child.runs[0].turn
            }
        );
        assert_eq!(task.summary.as_deref(), Some("Final child evidence"));
        assert!(
            !parent
                .entries
                .iter()
                .filter(|entry| entry.turn == notice.turn && entry.author == "user")
                .any(|entry| !entry.parts.is_empty())
        );
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        let request = fixture
            .parent
            .requests
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .clone();
        let input = request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rfind(|message| message["role"] == "user")
            .unwrap()["content"]
            .as_str()
            .unwrap();
        assert!(input.starts_with("Node background completion data follows as JSON."));
        let data: Value = serde_json::from_str(input.split_once('\n').unwrap().1).unwrap();
        assert_eq!(data[0]["output"]["response"], "Final child evidence");
        fixture.close().await;
    }
}

#[tokio::test]
async fn synchronous_spawn_waits_for_child() {
    for remote in [false, true] {
        let release = sailry_link::CancellationToken::new();
        let parent = Server::tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"role":"review","task":"Wait for the held response"}),
        )])
        .await;
        let child = Server::markdown_held("Final child evidence".into(), release.clone()).await;
        let fixture = Fixture::start(remote, parent, child).await;
        let active = fixture.push("Wait for the delegated task").await;
        let child = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        fixture.child.wait_count(1).await;
        let parent = history(&fixture.client, fixture.session.id).await;
        assert_eq!(status(&parent, active.id), Status::Running);
        assert_eq!(parent.children[0].run.status, Status::Running);
        assert!(results(&fixture.client, &parent).await.is_empty());
        assert_eq!(fixture.parent.requests.lock().unwrap().len(), 1);
        release.cancel();
        let parent = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(status(&parent, active.id), Status::Completed);
        assert_eq!(parent.children[0].run.status, Status::Completed);
        let output = results(&fixture.client, &parent).await;
        assert_eq!(output[0]["session"], json!(child.id));
        assert_eq!(output[0]["status"], "completed");
        assert_eq!(output[0]["response"], "Final child evidence");
        assert_eq!(fixture.parent.requests.lock().unwrap().len(), 2);
        fixture.close().await;
    }
}

#[tokio::test]
async fn default_background_child_inherits_permissions() {
    for remote in [false, true] {
        let spawn = plugin_tool("delegation", "spawn_agent");
        let parent = Server::prompt_tools(
            "Parent-only private context".into(),
            vec![
                ("tool_search".into(), json!({"query":spawn,"limit":1})),
                (
                    spawn.clone(),
                    json!({"task":"Independent default child","background":true}),
                ),
            ],
        )
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
        let page = finished(&fixture.client, child.id, page.runs[0].turn).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert!(page.approvals.is_empty());
        assert!(!page.entries.iter().any(|entry| {
            entry
                .parts
                .contains(&Part::Text("Parent-only private context".into()))
        }));
        let output = results(&fixture.client, &parent).await;
        assert_eq!(output.len(), 2, "{output:?}");
        assert_eq!(output[0]["tools"][0]["name"], spawn);
        assert_eq!(output[1]["session"], json!(child.id));
        fixture.close().await;
    }
}
