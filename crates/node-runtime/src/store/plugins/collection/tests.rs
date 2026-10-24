use super::*;

async fn fixture() -> Connection {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(include_str!("../../schema.sql"))
        .await
        .unwrap();
    db.execute(
        "INSERT INTO projects(id,name,path,appearance) VALUES('project','Fixture','fixture',?1)",
        [br#"{"icon":"folder","color":"blue"}"#.as_slice()],
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO worktrees(id,project,path,main) VALUES('fixture','project','fixture',1)",
        (),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO sessions(rowid,id,project,worktree,revision) VALUES(1,'session','project','fixture',1)",
        (),
    ).await
    .unwrap();
    db.execute_batch("INSERT INTO session_revisions(session,revision,worktree,config,roles) VALUES('session',1,'fixture','{}','[]'); INSERT INTO requests(caller,id,body,status) VALUES(X'00','request','{}','completed');").await
        .unwrap();
    db.execute(
        "INSERT INTO plugin_packages VALUES('unused','example',?1)",
        [b"{}".as_slice()],
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO plugin_settings VALUES('example',1,?1)",
        [b"{}".as_slice()],
    )
    .await
    .unwrap();
    db
}

async fn counts(db: &Connection) -> (i64, i64) {
    (async { let mut statement = db.prepare("SELECT (SELECT count(*) FROM plugin_packages),(SELECT count(*) FROM plugin_settings)").await?;
let row = statement.query_row(()).await?;
let mapped: turso::Result<_> = (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
mapped }).await
    .unwrap()
}

#[tokio::test]
async fn preserves_invalid_roots() {
    let mut db = fixture().await;
    db.execute("INSERT INTO turns(rowid,id,kind,plugins,session,revision,request,caller) VALUES(1,'turn','task',?1,'session',1,'request',X'00')", [b"invalid".as_slice()]).await.unwrap();
    assert!(metadata(&mut db).await.is_err());
    assert_eq!(counts(&db).await, (1, 1));
}

#[tokio::test]
async fn preserves_invalid_settings() {
    let mut db = fixture().await;
    db.execute("UPDATE plugin_settings SET body=X'00'", ())
        .await
        .unwrap();
    let reference = plugin::Reference {
        name: "example".into(),
        digest: "unused".into(),
        settings_revision: 1,
    };
    db.execute("INSERT INTO turns(rowid,id,kind,plugins,session,revision,request,caller) VALUES(1,'turn','task',?1,'session',1,'request',X'00')", [encode(&[reference]).unwrap()]).await.unwrap();
    assert!(metadata(&mut db).await.is_err());
    assert_eq!(counts(&db).await, (1, 1));
}

#[tokio::test]
async fn rolls_back_failure() {
    let mut db = fixture().await;
    db.execute_batch("CREATE TEMP TRIGGER fail_cleanup BEFORE DELETE ON plugin_settings BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").await.unwrap();
    assert!(metadata(&mut db).await.is_err());
    assert_eq!(counts(&db).await, (1, 1));
    db.execute_batch("DROP TRIGGER fail_cleanup;")
        .await
        .unwrap();
    assert!(metadata(&mut db).await.unwrap().is_empty());
    assert_eq!(counts(&db).await, (0, 0));
}

#[tokio::test]
async fn skips_native_callbacks_without_dropping_packages() {
    let mut db = fixture().await;
    for (sequence, (id, status, reference)) in [
        ("native-queued", "queued", serde_json::Value::Null),
        ("native-running", "running", serde_json::Value::Null),
        (
            "plugin-queued",
            "queued",
            serde_json::json!({"name":"example","digest":"unused","settings_revision":0}),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        db.execute("INSERT INTO dispatch_jobs(sequence,id,package,queue,status,body,callback,reference) VALUES(?4,?1,'','fixture',?2,'{}','{}',?3)", params![id,status,serde_json::to_vec(&reference).unwrap(),sequence as i64 + 1]).await.unwrap();
    }
    assert_eq!(
        metadata(&mut db).await.unwrap(),
        BTreeSet::from(["unused".into()])
    );
    assert_eq!(counts(&db).await, (1, 0));
}
