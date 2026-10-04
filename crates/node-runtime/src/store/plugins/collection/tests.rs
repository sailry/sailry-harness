use super::*;

fn fixture() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../../schema.sql")).unwrap();
    db.execute(
        "INSERT INTO projects(id,name,path,appearance) VALUES('project','Fixture','fixture',?1)",
        [br#"{"icon":"folder","color":"blue"}"#.as_slice()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO worktrees(id,project,path,main) VALUES('fixture','project','fixture',1)",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO sessions(id,project,worktree,revision) VALUES('session','project','fixture',1)",
        [],
    )
    .unwrap();
    db.execute_batch("INSERT INTO session_revisions(session,revision,worktree,config,roles) VALUES('session',1,'fixture','{}','[]'); INSERT INTO requests(caller,id,body,status) VALUES(X'00','request','{}','completed');")
        .unwrap();
    db.execute(
        "INSERT INTO plugin_packages VALUES('unused','example',?1)",
        [b"{}".as_slice()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO plugin_settings VALUES('example',1,?1)",
        [b"{}".as_slice()],
    )
    .unwrap();
    db
}

fn counts(db: &Connection) -> (i64, i64) {
    db.query_row(
        "SELECT (SELECT count(*) FROM plugin_packages),(SELECT count(*) FROM plugin_settings)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap()
}

#[test]
fn preserves_invalid_roots() {
    let mut db = fixture();
    db.execute("INSERT INTO turns(id,kind,plugins,session,revision,request,caller) VALUES('turn','task',?1,'session',1,'request',X'00')", [b"invalid".as_slice()]).unwrap();
    assert!(metadata(&mut db).is_err());
    assert_eq!(counts(&db), (1, 1));
}

#[test]
fn preserves_invalid_settings() {
    let mut db = fixture();
    db.execute("UPDATE plugin_settings SET body=X'00'", [])
        .unwrap();
    let reference = plugin::Reference {
        name: "example".into(),
        digest: "unused".into(),
        settings_revision: 1,
    };
    db.execute("INSERT INTO turns(id,kind,plugins,session,revision,request,caller) VALUES('turn','task',?1,'session',1,'request',X'00')", [encode(&[reference]).unwrap()]).unwrap();
    assert!(metadata(&mut db).is_err());
    assert_eq!(counts(&db), (1, 1));
}

#[test]
fn rolls_back_failure() {
    let mut db = fixture();
    db.execute_batch("CREATE TEMP TRIGGER fail_cleanup BEFORE DELETE ON plugin_settings BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    assert!(metadata(&mut db).is_err());
    assert_eq!(counts(&db), (1, 1));
    db.execute_batch("DROP TRIGGER fail_cleanup;").unwrap();
    assert!(metadata(&mut db).unwrap().is_empty());
    assert_eq!(counts(&db), (0, 0));
}
