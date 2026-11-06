use super::*;
use sailry_protocol::{SessionId, attachment::Spec};

#[tokio::test]
async fn confines_history_resolution() {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch("CREATE TABLE turns(id TEXT,session TEXT); CREATE TABLE conversation_turns(session TEXT,turn TEXT); CREATE TABLE turn_attachments(turn TEXT,attachment TEXT); CREATE TABLE attachments(id TEXT,worktree TEXT,body BLOB); CREATE TABLE agent_runs(turn TEXT,body BLOB); CREATE TABLE agent_pending(turn TEXT);").await.unwrap();
    let session = SessionId::new();
    let origin = TurnId::new();
    let current = TurnId::new();
    let attachment = Attachment {
        id: AttachmentId::new(),
        spec: Spec {
            worktree: WorktreeId::new(),
            name: "source.txt".into(),
            media_type: "text/plain".into(),
            size: 0,
            revision: String::new(),
        },
    };
    db.execute(
        "INSERT INTO turns VALUES(?1,?2)",
        params![current.to_string(), session.to_string()],
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO turn_attachments VALUES(?1,?2)",
        params![origin.to_string(), attachment.id.to_string()],
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO agent_runs VALUES(?1,?2)",
        params![origin.to_string(), r#"{"status":"completed"}"#],
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO attachments VALUES(?1,?2,?3)",
        params![
            attachment.id.to_string(),
            attachment.spec.worktree.to_string(),
            encode(&attachment).unwrap()
        ],
    )
    .await
    .unwrap();
    assert_eq!(
        resolve(&db, current, attachment.id).await.unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(get(&db, origin, attachment.id).await.unwrap(), attachment);
    assert_eq!(
        get(&db, current, attachment.id).await.unwrap_err().code,
        ErrorCode::NotFound
    );
    db.execute(
        "INSERT INTO conversation_turns VALUES(?1,?2)",
        params![session.to_string(), origin.to_string()],
    )
    .await
    .unwrap();
    assert_eq!(
        resolve(&db, current, attachment.id).await.unwrap(),
        attachment
    );
    assert!(!source(&db, current, attachment.id).await.unwrap().1);
    db.execute(
        "UPDATE agent_runs SET body=?1 WHERE turn=?2",
        params![
            format!("{{\"status\":\"continued\",\"joined\":\"{current}\"}}"),
            origin.to_string()
        ],
    )
    .await
    .unwrap();
    db.execute("INSERT INTO agent_pending VALUES(?1)", [origin.to_string()])
        .await
        .unwrap();
    let (resolved, current_input) = source(&db, current, attachment.id).await.unwrap();
    assert_eq!(resolved, attachment);
    assert!(current_input);
    db.execute(
        "DELETE FROM agent_pending WHERE turn=?1",
        [origin.to_string()],
    )
    .await
    .unwrap();
    for status in [
        "completed",
        "continued",
        "failed",
        "cancelled",
        "interrupted",
    ] {
        db.execute(
            "UPDATE agent_runs SET body=?1 WHERE turn=?2",
            params![format!("{{\"status\":\"{status}\"}}"), origin.to_string()],
        )
        .await
        .unwrap();
        assert_eq!(
            resolve(&db, current, attachment.id).await.unwrap(),
            attachment
        );
        db.execute("INSERT INTO agent_pending VALUES(?1)", [origin.to_string()])
            .await
            .unwrap();
        assert_eq!(
            resolve(&db, current, attachment.id).await.unwrap_err().code,
            ErrorCode::NotFound
        );
        db.execute(
            "DELETE FROM agent_pending WHERE turn=?1",
            [origin.to_string()],
        )
        .await
        .unwrap();
    }
    for status in ["queued", "running", "stopping"] {
        db.execute(
            "UPDATE agent_runs SET body=?1 WHERE turn=?2",
            params![format!("{{\"status\":\"{status}\"}}"), origin.to_string()],
        )
        .await
        .unwrap();
        assert_eq!(
            resolve(&db, current, attachment.id).await.unwrap_err().code,
            ErrorCode::NotFound
        );
    }
    // Own inputs remain available while executing, without exposing another turn.
    db.execute("UPDATE turn_attachments SET turn=?1", [current.to_string()])
        .await
        .unwrap();
    db.execute(
        "INSERT INTO conversation_turns VALUES(?1,?2)",
        params![session.to_string(), current.to_string()],
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO agent_runs VALUES(?1,?2)",
        params![current.to_string(), r#"{"status":"running"}"#],
    )
    .await
    .unwrap();
    assert_eq!(
        resolve(&db, current, attachment.id).await.unwrap(),
        attachment
    );
    assert!(source(&db, current, attachment.id).await.unwrap().1);
    db.execute(
        "DELETE FROM conversation_turns WHERE turn=?1",
        [current.to_string()],
    )
    .await
    .unwrap();
    assert_eq!(
        resolve(&db, current, attachment.id).await.unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        resolve(&db, TurnId::new(), attachment.id)
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(
        resolve(&db, current, AttachmentId::new())
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}

async fn classify(db: &Connection, turn: TurnId, id: AttachmentId) -> (&'static str, bool) {
    let found = miss(db, turn, id).await.unwrap();
    (found.reason, found.current)
}

#[tokio::test]
async fn classifies_unavailable_references() {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch("CREATE TABLE turns(id TEXT,session TEXT); CREATE TABLE conversation_turns(session TEXT,turn TEXT); CREATE TABLE turn_attachments(turn TEXT,attachment TEXT); CREATE TABLE attachments(id TEXT,worktree TEXT,body BLOB); CREATE TABLE agent_runs(turn TEXT,body BLOB); CREATE TABLE agent_pending(turn TEXT);").await.unwrap();
    let session = SessionId::new();
    let origin = TurnId::new();
    let current = TurnId::new();
    let id = AttachmentId::new();
    let fault = source(&db, current, id).await.unwrap_err();
    assert_eq!(fault.code, ErrorCode::NotFound);
    assert_eq!(fault.message, format!("attachment {id} is unavailable"));
    assert_eq!(get(&db, origin, id).await.unwrap_err(), fault);
    assert_eq!(classify(&db, current, id).await, ("missing_record", false));
    db.execute(
        "INSERT INTO attachments VALUES(?1,?2,?3)",
        params![id.to_string(), WorktreeId::new().to_string(), "{}"],
    )
    .await
    .unwrap();
    assert_eq!(classify(&db, current, id).await, ("outside_history", false));
    db.execute(
        "INSERT INTO turns VALUES(?1,?2)",
        params![current.to_string(), session.to_string()],
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO conversation_turns VALUES(?1,?2)",
        params![session.to_string(), origin.to_string()],
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO turn_attachments VALUES(?1,?2)",
        params![origin.to_string(), id.to_string()],
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO agent_runs VALUES(?1,?2)",
        params![origin.to_string(), r#"{"status":"completed"}"#],
    )
    .await
    .unwrap();
    db.execute("INSERT INTO agent_pending VALUES(?1)", [origin.to_string()])
        .await
        .unwrap();
    assert_eq!(classify(&db, current, id).await, ("pending_input", false));
    db.execute(
        "DELETE FROM agent_pending WHERE turn=?1",
        [origin.to_string()],
    )
    .await
    .unwrap();
    db.execute(
        "UPDATE agent_runs SET body=?1 WHERE turn=?2",
        params![r#"{"status":"running"}"#, origin.to_string()],
    )
    .await
    .unwrap();
    assert_eq!(classify(&db, current, id).await, ("active_origin", false));
    db.execute(
        "UPDATE agent_runs SET body=?1 WHERE turn=?2",
        params![
            format!("{{\"status\":\"continued\",\"joined\":\"{current}\"}}"),
            origin.to_string()
        ],
    )
    .await
    .unwrap();
    assert_eq!(
        classify(&db, current, id).await,
        ("visible_on_recheck", true)
    );
}
