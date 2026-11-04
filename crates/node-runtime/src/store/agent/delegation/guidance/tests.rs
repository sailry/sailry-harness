use super::super::fixture::{Fixture, execute, finish, invoke, profile, setup, start};
use super::*;
use sailry_protocol::attachment::{Attachment, Spec};
use serde_json::json;

async fn input(fixture: &Fixture, child: &Invocation) -> Input {
    let attachment = Attachment {
        id: AttachmentId::new(),
        spec: Spec {
            worktree: child.worktree,
            name: "note.txt".into(),
            media_type: "text/plain".into(),
            size: 0,
            revision: blake3::hash(b"").to_hex().to_string(),
        },
    };
    fixture
        .database
        .connection
        .execute(
            "INSERT INTO attachments(id,worktree,caller,body) VALUES(?1,?2,?3,?4)",
            params![
                attachment.id.to_string(),
                child.worktree.to_string(),
                &fixture.database.node.0[..],
                encode(&attachment).unwrap()
            ],
        )
        .await
        .unwrap();
    Input {
        text: "Preserve this guidance 中文 🙂".into(),
        attachments: vec![attachment.id],
        references: vec![reference::Reference {
            label: "源.txt".into(),
            target: reference::Target::File("源.txt".into()),
        }],
    }
}

fn prepare(fixture: &Fixture, child: &Invocation, message: Input) -> Request {
    Request::new(
        fixture.database.node,
        Command::GuideTurn {
            recipient: child.turn.id,
            expected_revision: child.turn.revision,
            message,
        },
    )
}

#[tokio::test]
async fn preserves_child_input() {
    let mut fixture = setup().await;
    for (index, scenario) in [
        "captures_input_and_call_ancestry",
        "preserves_stopped_queue",
        "removes_withdrawn_guidance",
        "recovery_preserves_input_without_replay",
        "validates_recipient_and_parent",
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            fixture.fresh_session().await;
        }
        eprintln!("scenario: {scenario}");
        match scenario {
            "captures_input_and_call_ancestry" => {
                Box::pin(async {
                    let role = profile(&mut fixture).await;
                    let parent = start(&mut fixture).await;
                    let child = invoke(
                        &mut fixture,
                        parent,
                        json!({"role":"review","task":"Inspect"}),
                    )
                    .await
                    .unwrap();
                    let message = input(&fixture, &child).await;
                    let captured = history::pages::read(
                        &fixture.database.connection,
                        child.turn.session,
                        None,
                        20,
                    )
                    .await
                    .unwrap();
                    assert_eq!(
                        captured.page.runs[0].input.as_ref().unwrap().message,
                        child.message,
                        "{scenario}"
                    );
                    assert!(
                        !serde_json::to_value(captured.page.runs[0].input.as_ref().unwrap())
                            .unwrap()
                            .to_string()
                            .contains(&role.instructions),
                        "{scenario}"
                    );
                    let request = prepare(&fixture, &child, message.clone());
                    let Output::QueuedTurn(source) =
                        execute(&mut fixture, request.clone()).await.unwrap()
                    else {
                        panic!("{scenario}: turn expected")
                    };
                    assert_eq!(
                        execute(&mut fixture, request).await.unwrap(),
                        Output::QueuedTurn(source.clone()),
                        "{scenario}"
                    );
                    assert_eq!(source.session, child.turn.session, "{scenario}");
                    assert_eq!(source.config, child.turn.config, "{scenario}");
                    assert_eq!(source.plugins, child.turn.plugins, "{scenario}");
                    assert_eq!(source.roles, child.turn.roles, "{scenario}");
                    let queued = history::pages::read(
                        &fixture.database.connection,
                        child.turn.session,
                        None,
                        20,
                    )
                    .await
                    .unwrap();
                    let queued = queued
                        .page
                        .runs
                        .iter()
                        .find(|run| run.turn == source.id)
                        .unwrap();
                    assert_eq!(queued.status, Status::Queued, "{scenario}");
                    assert_eq!(
                        queued.input.as_ref().unwrap().message,
                        message,
                        "{scenario}"
                    );
                    assert!(
                        queued_guidance(&fixture.database.connection, source.id)
                            .await
                            .unwrap(),
                        "{scenario}"
                    );
                    assert_eq!(
                        origin(&fixture.database.connection, source.id)
                            .await
                            .unwrap(),
                        origin(&fixture.database.connection, child.turn.id)
                            .await
                            .unwrap(),
                        "{scenario}"
                    );
                    assert_eq!(
                        queue::message(&fixture.database.connection, source.id)
                            .await
                            .unwrap()
                            .message,
                        message,
                        "{scenario}"
                    );
                    let duplicate = prepare(&fixture, &child, "Another guide".into());
                    let Output::QueuedTurn(second) =
                        execute(&mut fixture, duplicate).await.unwrap()
                    else {
                        panic!("{scenario}: turn expected")
                    };
                    assert_eq!(
                        queue::read(&fixture.database.connection, child.turn.session)
                            .await
                            .unwrap()
                            .items
                            .len(),
                        2,
                        "{scenario}"
                    );
                    let inputs =
                        runs::guidance::take(&mut fixture.database, child.turn.id, &fixture.events)
                            .await
                            .unwrap()
                            .unwrap();
                    assert_eq!(
                        runs::get(&fixture.database.connection, child.turn.id)
                            .await
                            .unwrap()
                            .status,
                        Status::Running,
                        "{scenario}"
                    );
                    assert_eq!(inputs.len(), 2, "{scenario}");
                    assert_eq!(inputs[0].turn.id, source.id, "{scenario}");
                    assert_eq!(inputs[0].message, message, "{scenario}");
                    assert_eq!(inputs[1].turn.id, second.id, "{scenario}");
                    assert_eq!(inputs[1].message.text, "Another guide", "{scenario}");
                    let claimed = history::pages::read(
                        &fixture.database.connection,
                        child.turn.session,
                        None,
                        20,
                    )
                    .await
                    .unwrap();
                    let claimed = claimed
                        .page
                        .runs
                        .iter()
                        .find(|run| run.turn == source.id)
                        .unwrap();
                    assert_eq!(claimed.status, Status::Continued, "{scenario}");
                    assert_eq!(claimed.joined, Some(child.turn.id), "{scenario}");
                    assert_eq!(
                        claimed.input.as_ref().unwrap().message,
                        message,
                        "{scenario}"
                    );
                    assert_eq!(
                        claimed.input.as_ref().unwrap().attachments[0].id,
                        message.attachments[0],
                        "{scenario}"
                    );
                    assert!(
                        !serde_json::to_value(claimed.input.as_ref().unwrap())
                            .unwrap()
                            .to_string()
                            .contains(&role.instructions),
                        "{scenario}"
                    );
                    assert_eq!(child.child.unwrap().role, Some(role), "{scenario}");
                    let summaries = fixture.read(None, 20).await.page.children;
                    assert_eq!(summaries.len(), 3, "{scenario}");
                    assert_eq!(summaries[0].run.status, Status::Running, "{scenario}");
                    assert_eq!(summaries[1].run.status, Status::Continued, "{scenario}");
                    assert_eq!(summaries[1].run.joined, Some(child.turn.id), "{scenario}");
                    assert_eq!(summaries[0].origin, summaries[1].origin, "{scenario}");
                    assert_eq!(summaries[2].run.joined, Some(child.turn.id), "{scenario}");
                    assert_eq!(summaries[2].run.status, Status::Continued, "{scenario}");
                })
                .await
            }
            "preserves_stopped_queue" => {
                Box::pin(async {
                    let parent = start(&mut fixture).await;
                    let child = invoke(&mut fixture, parent, json!({"role":null,"task":"Inspect"}))
                        .await
                        .unwrap();
                    runs::stop(&fixture.database.connection, child.turn.id)
                        .await
                        .unwrap();
                    finish(&mut fixture, child.turn.id).await;
                    let queue = queue::read(&fixture.database.connection, child.turn.session)
                        .await
                        .unwrap();
                    assert!(queue.paused, "{scenario}");
                    assert!(queue.items.is_empty(), "{scenario}");
                    for _ in 0..2 {
                        assert!(
                            runs::claim_guidance(
                                &mut fixture.database,
                                child.turn.id,
                                &fixture.events
                            )
                            .await
                            .unwrap()
                            .is_none(),
                            "{scenario}"
                        );
                        assert_eq!(
                            queue::read(&fixture.database.connection, child.turn.session)
                                .await
                                .unwrap(),
                            queue,
                            "{scenario}"
                        );
                    }
                })
                .await
            }
            "removes_withdrawn_guidance" => {
                Box::pin(async {
                    let parent = start(&mut fixture).await;
                    let child = invoke(&mut fixture, parent, json!({"role":null,"task":"Inspect"}))
                        .await
                        .unwrap();
                    let request = prepare(&fixture, &child, "Inspect the next file".into());
                    let Output::QueuedTurn(source) = execute(&mut fixture, request).await.unwrap()
                    else {
                        panic!("{scenario}: turn expected")
                    };
                    let item = queue::read(&fixture.database.connection, child.turn.session)
                        .await
                        .unwrap()
                        .items
                        .remove(0);
                    let request = Request::new(
                        fixture.database.node,
                        Command::WithdrawGuidance {
                            turn: source.id,
                            expected_revision: item.revision,
                        },
                    );
                    let Output::Queue(update) = execute(&mut fixture, request).await.unwrap()
                    else {
                        panic!("{scenario}: queue expected")
                    };
                    assert!(update.queue.items.is_empty(), "{scenario}");
                    assert_eq!(
                        runs::get(&fixture.database.connection, source.id)
                            .await
                            .unwrap()
                            .status,
                        Status::Cancelled,
                        "{scenario}"
                    );
                    assert_eq!(
                        runs::get(&fixture.database.connection, child.turn.id)
                            .await
                            .unwrap()
                            .status,
                        Status::Running,
                        "{scenario}"
                    );
                })
                .await
            }
            "recovery_preserves_input_without_replay" => {
                Box::pin(async {
                    let parent = start(&mut fixture).await;
                    let child = invoke(&mut fixture, parent, json!({"role":null,"task":"Inspect"}))
                        .await
                        .unwrap();
                    let message = input(&fixture, &child).await;
                    let request = prepare(&fixture, &child, message.clone());
                    let Output::QueuedTurn(source) = execute(&mut fixture, request).await.unwrap()
                    else {
                        panic!("{scenario}: turn expected")
                    };
                    {
                        let transaction = turso::transaction::Transaction::new_unchecked(
                            &fixture.database.connection,
                            turso::transaction::TransactionBehavior::Concurrent,
                        )
                        .await
                        .unwrap();
                        runs::recover(&transaction).await.unwrap();
                        transaction.commit().await.unwrap();
                    };
                    let queue = queue::read(&fixture.database.connection, child.turn.session)
                        .await
                        .unwrap();
                    assert!(queue.paused, "{scenario}");
                    assert_eq!(queue.items[0].guidance, None, "{scenario}");
                    assert_eq!(
                        runs::get(&fixture.database.connection, source.id)
                            .await
                            .unwrap()
                            .status,
                        Status::Queued,
                        "{scenario}"
                    );
                    assert_eq!(
                        queue::message(&fixture.database.connection, source.id)
                            .await
                            .unwrap()
                            .message,
                        message,
                        "{scenario}"
                    );
                    assert!(
                        queued_guidance(&fixture.database.connection, source.id)
                            .await
                            .unwrap(),
                        "{scenario}"
                    );
                    // The generic root worker must still exclude delegated work even without the pause fence.
                    queue::pause(&fixture.database.connection, child.turn.session, false)
                        .await
                        .unwrap();
                    assert!(
                        runs::claim(&mut fixture.database, &fixture.events)
                            .await
                            .unwrap()
                            .is_none(),
                        "{scenario}"
                    );
                    retain_guidance(&fixture.database.connection, None)
                        .await
                        .unwrap();
                    assert!(
                        queue::read(&fixture.database.connection, child.turn.session)
                            .await
                            .unwrap()
                            .paused,
                        "{scenario}"
                    );
                })
                .await
            }
            "validates_recipient_and_parent" => {
                Box::pin(async {
                    let parent = start(&mut fixture).await;
                    let child = invoke(&mut fixture, parent, json!({"role":null,"task":"Inspect"}))
                        .await
                        .unwrap();
                    let message: Input = "Guide".into();
                    let stale = runs::get(&fixture.database.connection, parent)
                        .await
                        .unwrap()
                        .revision
                        + 1;
                    for (recipient, revision, expected) in [
                        (parent, stale, ErrorCode::RevisionConflict),
                        (
                            child.turn.id,
                            child.turn.revision + 1,
                            ErrorCode::RevisionConflict,
                        ),
                    ] {
                        let request = Request::new(
                            fixture.database.node,
                            Command::GuideTurn {
                                recipient,
                                expected_revision: revision,
                                message: message.clone(),
                            },
                        );
                        assert_eq!(
                            execute(&mut fixture, request).await.unwrap_err().code,
                            expected,
                            "{scenario}"
                        );
                    }
                    runs::stop(&fixture.database.connection, parent)
                        .await
                        .unwrap();
                    let request = prepare(&fixture, &child, message);
                    assert_eq!(
                        execute(&mut fixture, request).await.unwrap_err().code,
                        ErrorCode::Conflict,
                        "{scenario}"
                    );
                    assert!(
                        queue::read(&fixture.database.connection, child.turn.session)
                            .await
                            .unwrap()
                            .items
                            .is_empty(),
                        "{scenario}"
                    );
                })
                .await
            }
            _ => unreachable!(),
        }
    }
}

#[tokio::test]
async fn explicit_stop_retains_readable_input() {
    for claimed in [false, true] {
        let mut fixture = setup().await;
        let parent = start(&mut fixture).await;
        let child = invoke(&mut fixture, parent, json!({"role":null,"task":"Inspect"}))
            .await
            .unwrap();
        let message = input(&fixture, &child).await;
        let request = prepare(&fixture, &child, message.clone());
        let Output::QueuedTurn(source) = execute(&mut fixture, request).await.unwrap() else {
            panic!("turn expected")
        };
        if claimed {
            let inputs =
                runs::guidance::take(&mut fixture.database, child.turn.id, &fixture.events)
                    .await
                    .unwrap()
                    .unwrap();
            assert_eq!(inputs[0].turn.id, source.id);
        }
        runs::stop(&fixture.database.connection, child.turn.id)
            .await
            .unwrap();
        finish(&mut fixture, child.turn.id).await;
        assert!(
            runs::claim_guidance(&mut fixture.database, child.turn.id, &fixture.events)
                .await
                .unwrap()
                .is_none()
        );
        let queue = queue::read(&fixture.database.connection, child.turn.session)
            .await
            .unwrap();
        assert!(queue.paused);
        assert_eq!(queue.items.len(), 1);
        assert_eq!(queue.items[0].guidance, None);
        let item = &queue.items[0];
        assert_eq!(
            queue::message(&fixture.database.connection, source.id)
                .await
                .unwrap()
                .message,
            message
        );
        assert_eq!(
            item.attachments
                .iter()
                .map(|attachment| attachment.id)
                .collect::<Vec<_>>(),
            message.attachments
        );
        assert_eq!(
            invoke(
                &mut fixture,
                parent,
                json!({"session":child.turn.session,"task":"Reuse while input is retained"}),
            )
            .await
            .err()
            .unwrap()
            .code,
            ErrorCode::Busy
        );
        for command in [
            Command::StartQueuedTurn { turn: source.id },
            Command::EditQueuedTurn {
                turn: source.id,
                expected_revision: item.revision,
                message: "Overwrite".into(),
            },
            Command::SendQueuedTurn {
                turn: source.id,
                expected_revision: item.revision,
            },
            Command::SetQueuePaused {
                session: child.turn.session,
                expected_revision: queue.revision,
                paused: false,
            },
        ] {
            let request = Request::new(fixture.database.node, command);
            assert_eq!(
                execute(&mut fixture, request).await.unwrap_err().code,
                ErrorCode::PermissionDenied
            );
        }
        let request = Request::new(
            fixture.database.node,
            Command::ReadQueuedTurn { turn: source.id },
        );
        let Output::QueuedMessage(read) = execute(&mut fixture, request).await.unwrap() else {
            panic!("queued input expected")
        };
        assert_eq!(read.message, message);
        let request = Request::new(
            fixture.database.node,
            Command::RemoveQueuedTurn {
                turn: source.id,
                expected_revision: item.revision,
            },
        );
        execute(&mut fixture, request).await.unwrap();
        assert!(
            queue::read(&fixture.database.connection, child.turn.session)
                .await
                .unwrap()
                .items
                .is_empty()
        );
    }
}
