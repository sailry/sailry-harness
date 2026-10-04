use super::*;
use serde_json::json;

#[test]
fn only_the_claimed_request_can_use_a_retained_version() {
    retained(Callback {
        scope: Scope::default(),
        completion: Completion::Command,
        command: Box::new(protocol::Command::CallPlugin {
            handler: "run".into(),
            input: json!(null),
        }),
        bindings: std::collections::BTreeMap::from([("/data/input".into(), "/payload".into())]),
    });
}

#[test]
fn ordinary_callbacks_retain_exact_authority() {
    retained(Callback {
        scope: Scope::default(),
        completion: Completion::Turn,
        command: Box::new(protocol::Command::StartSession(
            protocol::conversation::Start {
                project: None,
                worktree: None,
                config: None,
                title: "Scheduled".into(),
                message: "Initial".to_owned().into(),
            },
        )),
        bindings: std::collections::BTreeMap::from([(
            "/data/message/text".into(),
            "/payload/key".into(),
        )]),
    });
}

fn retained(callback: Callback) {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../../schema.sql")).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut old = super::super::tests::install(&db, directory.path(), "example");
    old.summary.digest = "a".repeat(64);
    let reference = old.summary.reference();
    let mut current = old.clone();
    current.summary.digest = "b".repeat(64);
    db.execute(
        "INSERT INTO plugin_packages(digest,name,body) VALUES(?1,?2,?3)",
        params![reference.digest, reference.name, encode(&old).unwrap()],
    )
    .unwrap();
    db.execute(
        "UPDATE plugins SET body=?2 WHERE name=?1",
        params![reference.name, encode(&current).unwrap()],
    )
    .unwrap();
    let handler = Handler {
        name: "receive".into(),
        revision: 1,
        enabled: true,
        source: Source {
            package: reference.name.clone(),
            topic: "tick".into(),
        },
        queue: "queue".into(),
        callback,
    };
    let event = Event {
        id: protocol::EventId::new(),
        source: handler.source.clone(),
        payload: json!({"key":"original"}),
        timestamp_ms: 1,
        schedule: None,
        scheduled_ms: None,
    };
    let job = events::enqueue(&db, &reference, &handler, &event).unwrap();
    let node = protocol::NodeId([91; 32]);
    let (mut work, _) = claim(&db, node, 2).unwrap();
    let original = work.pop().unwrap().request;
    assert_eq!(
        package(&db, node, node, &original)
            .unwrap()
            .unwrap()
            .summary
            .reference(),
        reference
    );
    assert!(
        package(&db, node, protocol::NodeId([92; 32]), &original)
            .unwrap()
            .is_none()
    );
    let mut forged = original.clone();
    forged.command = protocol::Command::CallPlugin {
        handler: "run".into(),
        input: json!({"key":"forged"}),
    };
    assert_eq!(
        package(&db, node, node, &forged).unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    forged = original.clone();
    forged.plugin.as_mut().unwrap().worktree = Some(protocol::WorktreeId::new());
    assert_eq!(
        package(&db, node, node, &forged).unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    current.summary.enabled = false;
    db.execute("UPDATE plugins SET body=?1", [encode(&current).unwrap()])
        .unwrap();
    assert_eq!(
        package(&db, node, node, &original).unwrap_err().code,
        ErrorCode::NotConfigured
    );
    finish(&db, job.id, Ok(())).unwrap();
    assert!(package(&db, node, node, &original).unwrap().is_none());
}
