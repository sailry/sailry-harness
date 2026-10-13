use super::super::fixture::{Fixture, execute, finish, invoke, profile, setup, start};
use super::*;
use sailry_protocol::attachment::{Attachment, Spec};
use serde_json::json;

fn input(fixture: &Fixture, child: &Invocation) -> Input {
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

#[test]
fn captures_input_and_call_ancestry() {
    let mut fixture = setup();
    let role = profile(&mut fixture);
    let parent = start(&mut fixture);
    let child = invoke(
        &mut fixture,
        parent,
        json!({"role":"review","task":"Inspect"}),
    )
    .unwrap();
    let message = input(&fixture, &child);
    let captured =
        history::pages::read(&fixture.database.connection, child.turn.session, None, 20).unwrap();
    assert_eq!(
        captured.page.runs[0].input.as_ref().unwrap().message,
        child.message
    );
    assert!(
        !serde_json::to_value(captured.page.runs[0].input.as_ref().unwrap())
            .unwrap()
            .to_string()
            .contains(&role.instructions)
    );
    let request = prepare(&fixture, &child, message.clone());
    let Output::QueuedTurn(source) = execute(&mut fixture, request.clone()).unwrap() else {
        panic!("turn expected")
    };
    assert_eq!(
        execute(&mut fixture, request).unwrap(),
        Output::QueuedTurn(source.clone())
    );
    assert_eq!(source.session, child.turn.session);
    assert_eq!(source.config, child.turn.config);
    assert_eq!(source.plugins, child.turn.plugins);
    assert_eq!(source.roles, child.turn.roles);
    let queued =
        history::pages::read(&fixture.database.connection, child.turn.session, None, 20).unwrap();
    let queued = queued
        .page
        .runs
        .iter()
        .find(|run| run.turn == source.id)
        .unwrap();
    assert_eq!(queued.status, Status::Queued);
    assert!(queued.input.is_none());
    assert!(queued_guidance(&fixture.database.connection, source.id).unwrap());
    assert_eq!(
        origin(&fixture.database.connection, source.id).unwrap(),
        origin(&fixture.database.connection, child.turn.id).unwrap()
    );
    assert_eq!(
        queue::message(&fixture.database.connection, source.id)
            .unwrap()
            .message,
        message
    );
    let duplicate = prepare(&fixture, &child, "Another guide".into());
    assert_eq!(
        execute(&mut fixture, duplicate).unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(
        queue::read(&fixture.database.connection, child.turn.session)
            .unwrap()
            .items
            .len(),
        1
    );
    assert!(
        queue::guidance::yield_run(&mut fixture.database, child.turn.id, &fixture.events).unwrap()
    );
    assert_eq!(
        runs::get(&fixture.database.connection, child.turn.id)
            .unwrap()
            .status,
        Status::Stopping
    );
    finish(&mut fixture, child.turn.id);
    assert_eq!(
        runs::get(&fixture.database.connection, child.turn.id)
            .unwrap()
            .status,
        Status::Cancelled
    );
    let next = runs::claim_guidance(&mut fixture.database, child.turn.id, &fixture.events)
        .unwrap()
        .unwrap();
    assert_eq!(next.turn.id, source.id);
    assert_eq!(next.message, message);
    let claimed =
        history::pages::read(&fixture.database.connection, child.turn.session, None, 20).unwrap();
    let claimed = claimed
        .page
        .runs
        .iter()
        .find(|run| run.turn == source.id)
        .unwrap();
    assert_eq!(claimed.status, Status::Running);
    assert_eq!(claimed.input.as_ref().unwrap().message, message);
    assert_eq!(
        claimed.input.as_ref().unwrap().attachments[0].id,
        message.attachments[0]
    );
    assert!(
        !serde_json::to_value(claimed.input.as_ref().unwrap())
            .unwrap()
            .to_string()
            .contains(&role.instructions)
    );
    assert_eq!(next.child.unwrap().role, Some(role));
    let summaries = fixture.read(None, 20).page.children;
    assert_eq!(summaries.len(), 2);
    assert_eq!(summaries[0].run.status, Status::Cancelled);
    assert_eq!(summaries[1].run.status, Status::Running);
    assert_eq!(summaries[0].origin, summaries[1].origin);
}

#[test]
fn preserves_stopped_queue() {
    let mut fixture = setup();
    let parent = start(&mut fixture);
    let child = invoke(&mut fixture, parent, json!({"role":null,"task":"Inspect"})).unwrap();
    runs::stop(&fixture.database.connection, child.turn.id).unwrap();
    finish(&mut fixture, child.turn.id);
    let queue = queue::read(&fixture.database.connection, child.turn.session).unwrap();
    assert!(queue.paused);
    assert!(queue.items.is_empty());
    for _ in 0..2 {
        assert!(
            runs::claim_guidance(&mut fixture.database, child.turn.id, &fixture.events)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            queue::read(&fixture.database.connection, child.turn.session).unwrap(),
            queue
        );
    }
}

#[test]
fn explicit_stop_retains_readable_input() {
    for yielded in [false, true] {
        let mut fixture = setup();
        let parent = start(&mut fixture);
        let child = invoke(&mut fixture, parent, json!({"role":null,"task":"Inspect"})).unwrap();
        let message = input(&fixture, &child);
        let request = prepare(&fixture, &child, message.clone());
        let Output::QueuedTurn(source) = execute(&mut fixture, request).unwrap() else {
            panic!("turn expected")
        };
        if yielded {
            assert!(
                queue::guidance::yield_run(&mut fixture.database, child.turn.id, &fixture.events)
                    .unwrap()
            );
        }
        runs::stop(&fixture.database.connection, child.turn.id).unwrap();
        finish(&mut fixture, child.turn.id);
        assert!(
            runs::claim_guidance(&mut fixture.database, child.turn.id, &fixture.events)
                .unwrap()
                .is_none()
        );
        let queue = queue::read(&fixture.database.connection, child.turn.session).unwrap();
        assert!(queue.paused);
        assert_eq!(queue.items.len(), 1);
        assert_eq!(queue.items[0].guidance, None);
        let item = &queue.items[0];
        assert_eq!(
            queue::message(&fixture.database.connection, source.id)
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
                execute(&mut fixture, request).unwrap_err().code,
                ErrorCode::PermissionDenied
            );
        }
        let request = Request::new(
            fixture.database.node,
            Command::ReadQueuedTurn { turn: source.id },
        );
        let Output::QueuedMessage(read) = execute(&mut fixture, request).unwrap() else {
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
        execute(&mut fixture, request).unwrap();
        assert!(
            queue::read(&fixture.database.connection, child.turn.session)
                .unwrap()
                .items
                .is_empty()
        );
    }
}

#[test]
fn recovery_preserves_input_without_replay() {
    let mut fixture = setup();
    let parent = start(&mut fixture);
    let child = invoke(&mut fixture, parent, json!({"role":null,"task":"Inspect"})).unwrap();
    let message = input(&fixture, &child);
    let request = prepare(&fixture, &child, message.clone());
    let Output::QueuedTurn(source) = execute(&mut fixture, request).unwrap() else {
        panic!("turn expected")
    };
    runs::recover(&fixture.database.connection).unwrap();
    let queue = queue::read(&fixture.database.connection, child.turn.session).unwrap();
    assert!(queue.paused);
    assert_eq!(queue.items[0].guidance, None);
    assert_eq!(
        runs::get(&fixture.database.connection, source.id)
            .unwrap()
            .status,
        Status::Queued
    );
    assert_eq!(
        queue::message(&fixture.database.connection, source.id)
            .unwrap()
            .message,
        message
    );
    assert!(queued_guidance(&fixture.database.connection, source.id).unwrap());
    // The generic root worker must still exclude delegated work even without the pause fence.
    queue::pause(&fixture.database.connection, child.turn.session, false).unwrap();
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .is_none()
    );
    retain_guidance(&fixture.database.connection, None).unwrap();
    assert!(
        queue::read(&fixture.database.connection, child.turn.session)
            .unwrap()
            .paused
    );
}

#[test]
fn validates_recipient_and_parent() {
    let mut fixture = setup();
    let parent = start(&mut fixture);
    let child = invoke(&mut fixture, parent, json!({"role":null,"task":"Inspect"})).unwrap();
    let message: Input = "Guide".into();
    let stale = runs::get(&fixture.database.connection, parent)
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
        assert_eq!(execute(&mut fixture, request).unwrap_err().code, expected);
    }
    runs::stop(&fixture.database.connection, parent).unwrap();
    let request = prepare(&fixture, &child, message);
    assert_eq!(
        execute(&mut fixture, request).unwrap_err().code,
        ErrorCode::Conflict
    );
    assert!(
        queue::read(&fixture.database.connection, child.turn.session)
            .unwrap()
            .items
            .is_empty()
    );
}
