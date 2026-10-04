use super::*;
use sailry_protocol::{SessionId, attachment::Spec};

#[test]
fn confines_history_resolution() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE turns(id TEXT,session TEXT); CREATE TABLE conversation_turns(session TEXT,turn TEXT); CREATE TABLE turn_attachments(turn TEXT,attachment TEXT); CREATE TABLE attachments(id TEXT,worktree TEXT,body BLOB);").unwrap();
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
    assert_eq!(
        resolve(&db, TurnId::new(), attachment.id).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        resolve(&db, current, AttachmentId::new()).unwrap_err().code,
        ErrorCode::NotFound
    );
}
