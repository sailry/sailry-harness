use super::*;
use sailry_protocol::{WorktreeId, conversation::Run};

fn fixture() -> (Connection, TurnId, WorktreeId, std::path::PathBuf) {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE agent_runs(turn TEXT PRIMARY KEY,body BLOB); CREATE TABLE worktrees(id TEXT PRIMARY KEY,path TEXT)").unwrap();
    let turn = TurnId::new();
    let worktree = WorktreeId::new();
    let root = std::env::temp_dir().join("registered-sqlite-worktree");
    let run = Run {
        worktree,
        turn,
        kind: RunKind::Task,
        session: SessionId::new(),
        sequence: 1,
        revision: 1,
        status: Status::Running,
        error: None,
        started_ms: Some(1),
        finished_ms: None,
        origin: None,
        continuation: None,
        input: None,
    };
    db.execute(
        "INSERT INTO agent_runs VALUES(?1,?2)",
        params![turn.to_string(), encode(&run).unwrap()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO worktrees VALUES(?1,?2)",
        params![worktree.to_string(), root.to_str().unwrap()],
    )
    .unwrap();
    (db, turn, worktree, root)
}

#[test]
fn resolves_only_supplied_relative_paths() {
    let (db, turn, _, root) = fixture();
    for (path, relative) in [
        ("sales.db", "sales.db"),
        ("./sales.db", "sales.db"),
        ("data/sales.db", "data/sales.db"),
    ] {
        assert_eq!(
            sqlite_path(&db, turn, path.into()).unwrap(),
            root.join(relative).to_str().unwrap()
        );
    }
    let absolute = std::env::temp_dir().join("external-sales.db");
    assert_eq!(
        sqlite_path(&db, TurnId::new(), absolute.to_str().unwrap().into()).unwrap(),
        absolute.to_str().unwrap()
    );
}

#[test]
fn rejects_escape_and_missing_registration() {
    let (db, turn, worktree, _) = fixture();
    for path in [
        "../sales.db",
        "data/../sales.db",
        "././sales.db",
        "",
        "a//b",
        "~user/sales\0.db",
        ".sailry/worktrees/other/sales.db",
    ] {
        assert!(sqlite_path(&db, turn, path.into()).is_err(), "{path:?}");
    }
    db.execute("DELETE FROM worktrees WHERE id=?1", [worktree.to_string()])
        .unwrap();
    assert_eq!(
        sqlite_path(&db, turn, "sales.db".into()).unwrap_err().code,
        ErrorCode::NotFound
    );
    db.execute(
        "INSERT INTO worktrees VALUES(?1,'relative-root')",
        [worktree.to_string()],
    )
    .unwrap();
    assert_eq!(
        sqlite_path(&db, turn, "sales.db".into()).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
}
