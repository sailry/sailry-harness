use super::*;
use sailry_protocol::{ProviderId, SessionId, WorktreeId};
use serde_json::json;

async fn fixture() -> Connection {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(include_str!("../../../schema.sql"))
        .await
        .unwrap();
    db.execute("BEGIN", ()).await.unwrap();
    db
}

/// An installed copy of the former bundled delegation package.
async fn installed(db: &Connection, budget: &str) -> Info {
    let info: Info = serde_json::from_value(json!({
        "summary":{"name":"delegation","revision":1,"settings_revision":0,"enabled":true,"digest":"a".repeat(64),"version":"0.1.29"},
        "origin":{"kind":"bundled"},
        "extension":{"api_version":"v1","scope":"host","actions":["agents.delegate"]},
        "settings":{"$schema":plugin::settings::SCHEMA,"type":"object","additionalProperties":false,
            "properties":{"child_concurrency":{"type":"string","default":"8","x-sailry-child-concurrency":true}},"required":["child_concurrency"]}
    }))
    .unwrap();
    save(db, &info, true).await.unwrap();
    let mut state = settings::read(db, &info.summary.reference()).await.unwrap();
    state
        .values
        .insert("child_concurrency".into(), json!(budget));
    settings::save(db, &state.package, &state.values, &Default::default())
        .await
        .unwrap();
    required(db, "delegation").await.unwrap()
}

fn config(mode: &str, options: serde_json::Value) -> serde_json::Value {
    json!({"provider":ProviderId::new(),"model":"fixture","mode":mode,"options":options,"permission":"ask","credential":null})
}

async fn session(db: &Connection, row: i64, config: &serde_json::Value) -> SessionId {
    let id = SessionId::new();
    let worktree = WorktreeId::new();
    db.execute(
        "INSERT INTO worktrees(id,path,main) VALUES(?1,?2,0)",
        params![worktree.to_string(), format!("/fixture/{worktree}")],
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO sessions(rowid,id,worktree,revision) VALUES(?3,?1,?2,1)",
        params![id.to_string(), worktree.to_string(), row],
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO session_revisions(session,revision,worktree,config,roles) VALUES(?1,1,?2,?3,?4)",
        params![
            id.to_string(),
            worktree.to_string(),
            encode(config).unwrap(),
            encode(&sailry_protocol::role::Snapshot::default()).unwrap()
        ],
    )
    .await
    .unwrap();
    id
}

async fn revision(db: &Connection, id: SessionId, revision: i64) -> SessionConfig {
    let mut statement = db
        .prepare("SELECT config FROM session_revisions WHERE session=?1 AND revision=?2")
        .await
        .unwrap();
    let row = statement
        .query_row(params![id.to_string(), revision])
        .await
        .unwrap();
    serde_json::from_slice(&row.get::<Vec<u8>>(0).unwrap()).unwrap()
}

#[tokio::test]
async fn selects_flagship_for_stored_multitask() {
    let db = fixture().await;
    installed(&db, "8").await;
    let flagship = session(
        &db,
        1,
        &config(
            "code",
            json!({"delegation":{"proactive":true,"mode":"multitask"}}),
        ),
    )
    .await;
    let planning = session(
        &db,
        2,
        &config(
            "plan",
            json!({"delegation":{"proactive":false,"mode":"multitask"}}),
        ),
    )
    .await;
    let standard = session(
        &db,
        3,
        &config("code", json!({"delegation":{"proactive":true,"mode":""}})),
    )
    .await;
    db.execute(
        "INSERT INTO defaults(singleton,revision,config) VALUES(1,4,?1)",
        [encode(&config("code", json!({"delegation":{"mode":"multitask"}}))).unwrap()],
    )
    .await
    .unwrap();
    assert!(pending(&db).await.unwrap());
    let changes = retire(&db).await.unwrap();

    let converted = crate::store::commands::session(&db, flagship)
        .await
        .unwrap();
    assert_eq!(converted.revision, 2);
    assert_eq!(converted.config.mode, WorkMode::Flagship);
    assert_eq!(
        converted.config.options["delegation"]["proactive"],
        json!(true)
    );
    assert!(!converted.config.options["delegation"].contains_key("mode"));
    // The sent revision keeps the package selection it was admitted with.
    let original = revision(&db, flagship, 1).await;
    assert_eq!(original.mode, WorkMode::Code);
    assert_eq!(original.options["delegation"]["mode"], json!("multitask"));

    let planned = crate::store::commands::session(&db, planning)
        .await
        .unwrap();
    assert_eq!(planned.revision, 2);
    assert_eq!(planned.config.mode, WorkMode::Plan);
    assert!(!planned.config.options["delegation"].contains_key("mode"));

    let unchanged = crate::store::commands::session(&db, standard)
        .await
        .unwrap();
    assert_eq!(unchanged.revision, 1);
    assert_eq!(unchanged.config.mode, WorkMode::Code);

    let defaults = crate::store::commands::defaults(&db).await.unwrap();
    assert_eq!(defaults.revision, 5);
    let config = defaults.config.unwrap();
    assert_eq!(config.mode, WorkMode::Flagship);
    assert!(!config.options.contains_key("delegation"));

    assert!(get(&db, "delegation").await.unwrap().is_none());
    assert_eq!(
        changes
            .iter()
            .filter(|event| matches!(event, Event::SessionChanged(_)))
            .count(),
        2
    );
    assert!(
        changes
            .iter()
            .any(|event| matches!(event, Event::DefaultsChanged(_)))
    );
    assert!(
        changes
            .iter()
            .any(|event| matches!(event, Event::PluginRemoved { name } if name == "delegation"))
    );
    assert!(!pending(&db).await.unwrap());
    assert!(retire(&db).await.unwrap().is_empty());
}

#[tokio::test]
async fn imports_child_concurrency() {
    let db = fixture().await;
    installed(&db, "3").await;
    retire(&db).await.unwrap();
    let settings = crate::store::teams::settings::read(&db).await.unwrap();
    assert_eq!((settings.revision, settings.child_concurrency), (1, 3));
}

#[tokio::test]
async fn keeps_custom_copies() {
    let db = fixture().await;
    let mut info = installed(&db, "3").await;
    info.origin = Some(plugin::Origin::Directory);
    save(&db, &info, true).await.unwrap();
    assert!(!pending(&db).await.unwrap());
    assert!(retire(&db).await.unwrap().is_empty());
    assert!(get(&db, "delegation").await.unwrap().is_some());
    assert_eq!(
        crate::store::teams::settings::read(&db).await.unwrap(),
        Default::default()
    );
}
