use super::*;
use serde_json::json;

#[tokio::test]
async fn retained_version_requires_claimed_request() {
    retained(Callback {
        scope: Scope::default(),
        completion: Completion::Command,
        command: Box::new(protocol::Command::CallPlugin {
            handler: "run".into(),
            input: json!(null),
        }),
        bindings: std::collections::BTreeMap::from([("/data/input".into(), "/payload".into())]),
    })
    .await;
}

#[tokio::test]
async fn ordinary_callbacks_retain_exact_authority() {
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
    })
    .await;
}

async fn retained(callback: Callback) {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(include_str!("../../schema.sql"))
        .await
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut old = super::super::tests::install(&db, directory.path(), "example").await;
    old.summary.digest = "a".repeat(64);
    let reference = old.summary.reference();
    let mut current = old.clone();
    current.summary.digest = "b".repeat(64);
    db.execute(
        "INSERT INTO plugin_packages(digest,name,body) VALUES(?1,?2,?3)",
        params![
            reference.digest.as_str(),
            reference.name.as_str(),
            encode(&old).unwrap()
        ],
    )
    .await
    .unwrap();
    db.execute(
        "UPDATE plugins SET body=?2 WHERE name=?1",
        params![reference.name.as_str(), encode(&current).unwrap()],
    )
    .await
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
    db.execute_batch("BEGIN CONCURRENT").await.unwrap();
    let job = events::enqueue(&db, Some(&reference), &handler, &event)
        .await
        .unwrap();
    let node = protocol::NodeId([91; 32]);
    let (mut work, _) = claim(&db, node, 2).await.unwrap();
    let original = work.pop().unwrap().request;
    assert_eq!(
        package(&db, node, node, &original)
            .await
            .unwrap()
            .unwrap()
            .summary
            .reference(),
        reference
    );
    assert!(
        package(&db, node, protocol::NodeId([92; 32]), &original)
            .await
            .unwrap()
            .is_none()
    );
    let mut forged = original.clone();
    forged.command = protocol::Command::CallPlugin {
        handler: "run".into(),
        input: json!({"key":"forged"}),
    };
    assert_eq!(
        package(&db, node, node, &forged).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    forged = original.clone();
    forged.plugin.as_mut().unwrap().worktree = Some(protocol::WorktreeId::new());
    assert_eq!(
        package(&db, node, node, &forged).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    current.summary.enabled = false;
    db.execute("UPDATE plugins SET body=?1", [encode(&current).unwrap()])
        .await
        .unwrap();
    assert_eq!(
        package(&db, node, node, &original).await.unwrap_err().code,
        ErrorCode::NotConfigured
    );
    finish(&db, job.id, Ok(())).await.unwrap();
    assert!(package(&db, node, node, &original).await.unwrap().is_none());
    db.execute_batch("COMMIT").await.unwrap();
}
