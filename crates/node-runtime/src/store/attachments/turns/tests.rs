use super::*;
use sailry_protocol::{SessionId, attachment::Spec};

#[test]
fn confines_history_resolution() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE turns(id TEXT,session TEXT); CREATE TABLE conversation_turns(session TEXT,turn TEXT); CREATE TABLE turn_attachments(turn TEXT,attachment TEXT); CREATE TABLE attachments(id TEXT,worktree TEXT,body BLOB); CREATE TABLE agent_runs(turn TEXT,body BLOB); CREATE TABLE agent_pending(turn TEXT);").unwrap();
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
    .unwrap();
    db.execute(
        "INSERT INTO turn_attachments VALUES(?1,?2)",
        params![origin.to_string(), attachment.id.to_string()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO agent_runs VALUES(?1,?2)",
        params![origin.to_string(), r#"{"status":"completed"}"#],
    )
    .unwrap();
    db.execute(
        "INSERT INTO attachments VALUES(?1,?2,?3)",
        params![
            attachment.id.to_string(),
            attachment.spec.worktree.to_string(),
            encode(&attachment).unwrap()
        ],
    )
    .unwrap();
    assert_eq!(
        resolve(&db, current, attachment.id).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(get(&db, origin, attachment.id).unwrap(), attachment);
    assert_eq!(
        get(&db, current, attachment.id).unwrap_err().code,
        ErrorCode::NotFound
    );
    db.execute(
        "INSERT INTO conversation_turns VALUES(?1,?2)",
        params![session.to_string(), origin.to_string()],
    )
    .unwrap();
    assert_eq!(resolve(&db, current, attachment.id).unwrap(), attachment);
    for status in ["completed", "failed", "cancelled"] {
        db.execute(
            "UPDATE agent_runs SET body=?1 WHERE turn=?2",
            params![format!("{{\"status\":\"{status}\"}}"), origin.to_string()],
        )
        .unwrap();
        assert_eq!(resolve(&db, current, attachment.id).unwrap(), attachment);
        db.execute("INSERT INTO agent_pending VALUES(?1)", [origin.to_string()])
            .unwrap();
        assert_eq!(
            resolve(&db, current, attachment.id).unwrap_err().code,
            ErrorCode::NotFound
        );
        db.execute(
            "DELETE FROM agent_pending WHERE turn=?1",
            [origin.to_string()],
        )
        .unwrap();
    }
    for status in ["queued", "running", "stopping", "interrupted", "continued"] {
        db.execute(
            "UPDATE agent_runs SET body=?1 WHERE turn=?2",
            params![format!("{{\"status\":\"{status}\"}}"), origin.to_string()],
        )
        .unwrap();
        assert_eq!(
            resolve(&db, current, attachment.id).unwrap_err().code,
            ErrorCode::NotFound
        );
    }
    // Own inputs remain available while executing, without exposing another turn.
    db.execute("UPDATE turn_attachments SET turn=?1", [current.to_string()])
        .unwrap();
    db.execute(
        "INSERT INTO conversation_turns VALUES(?1,?2)",
        params![session.to_string(), current.to_string()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO agent_runs VALUES(?1,?2)",
        params![current.to_string(), r#"{"status":"running"}"#],
    )
    .unwrap();
    assert_eq!(resolve(&db, current, attachment.id).unwrap(), attachment);
    db.execute(
        "DELETE FROM conversation_turns WHERE turn=?1",
        [current.to_string()],
    )
    .unwrap();
    assert_eq!(
        resolve(&db, current, attachment.id).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        resolve(&db, TurnId::new(), attachment.id).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        resolve(&db, current, AttachmentId::new()).unwrap_err().code,
        ErrorCode::NotFound
    );
}
