use super::*;
use sailry_protocol::{ProviderId, SessionId, WorktreeId};
use serde_json::json;
use std::collections::BTreeMap;

fn config() -> SessionConfig {
    serde_json::from_value(json!({"provider":ProviderId::new(),"model":"fixture","mode":"code","permission":"ask","credential":null})).unwrap()
}

async fn fixture() -> (Connection, Info) {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(include_str!("../../schema.sql"))
        .await
        .unwrap();
    let info: Info=serde_json::from_value(json!({
        "summary":{"name":"example","revision":1,"settings_revision":0,"enabled":true,"digest":"a".repeat(64)},
        "extension":{"api_version":"v1","conversation_options":{"$schema":plugin::settings::SCHEMA,"type":"object","additionalProperties":false,
            "properties":{"enabled":{"type":"boolean","default":false,"x-sailry-import":"prior_flag"}},"required":["enabled"]}}
    })).unwrap();
    super::super::save(&db, &info, true).await.unwrap();
    (db, info)
}

#[tokio::test]
async fn confines_changes_to_declared_owner() {
    let (db, info) = fixture().await;
    let before = config();
    let mut after = before.clone();
    after.options.insert(
        "example".into(),
        BTreeMap::from([("enabled".into(), json!(true))]),
    );
    validate(&db, Some(&before), &after, Some(&info.summary.reference()))
        .await
        .unwrap();
    let mut foreign = info.summary.reference();
    foreign.name = "other".into();
    assert_eq!(
        validate(&db, Some(&before), &after, Some(&foreign))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    let mut stale = info.summary.reference();
    stale.settings_revision += 1;
    assert_eq!(
        validate(&db, Some(&before), &after, Some(&stale))
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    after
        .options
        .get_mut("example")
        .unwrap()
        .insert("unknown".into(), json!(true));
    assert_eq!(
        validate(&db, Some(&before), &after, None)
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    let mut retained = before.clone();
    retained.options.insert(
        "removed-package".into(),
        BTreeMap::from([("choice".into(), json!(true))]),
    );
    let mut changed = retained.clone();
    changed.mode = sailry_protocol::WorkMode::Plan;
    validate(&db, Some(&retained), &changed, None)
        .await
        .unwrap();
}

#[tokio::test]
async fn imports_only_current_explicit_preferences() {
    let (db, info) = fixture().await;
    let mut ids = vec![];
    let mut originals = vec![];
    for (row, (flag, chosen)) in [(true, None), (true, Some(false)), (false, None)]
        .into_iter()
        .enumerate()
    {
        let id = SessionId::new();
        let worktree = WorktreeId::new();
        db.execute(
            "INSERT INTO worktrees(id,path,main) VALUES(?1,?2,0)",
            params![worktree.to_string(), format!("/fixture/{worktree}")],
        )
        .await
        .unwrap();
        let mut raw = serde_json::to_value(config()).unwrap();
        raw["prior_flag"] = json!(flag);
        if let Some(chosen) = chosen {
            raw["options"] = json!({"example":{"enabled":chosen}});
        }
        let bytes = encode(&raw).unwrap();
        db.execute(
            "INSERT INTO sessions(rowid,id,worktree,revision) VALUES(?3,?1,?2,1)",
            params![id.to_string(), worktree.to_string(), row as i64 + 1],
        )
        .await
        .unwrap();
        db.execute("INSERT INTO session_revisions(session,revision,worktree,config,roles) VALUES(?1,1,?2,?3,?4)",params![id.to_string(),worktree.to_string(),bytes.as_slice(),encode(&sailry_protocol::role::Snapshot::default()).unwrap()]).await.unwrap();
        ids.push(id);
        originals.push(bytes);
    }
    // This historical value must not leak into a newer configuration lacking the source.
    let mut current = super::super::super::commands::session(&db, ids[2])
        .await
        .unwrap();
    current.revision = 2;
    db.execute(
        "UPDATE sessions SET revision=2 WHERE id=?1",
        [ids[2].to_string()],
    )
    .await
    .unwrap();
    db.execute("INSERT INTO session_revisions(session,revision,worktree,config,roles) VALUES(?1,2,?2,?3,?4)",params![current.id.to_string(),current.worktree.to_string(),encode(&config()).unwrap(),encode(&current.roles).unwrap()]).await.unwrap();
    let changes = migrate(&db, &info).await.unwrap();
    assert_eq!(changes.len(), 1);
    let migrated = super::super::super::commands::session(&db, ids[0])
        .await
        .unwrap();
    assert_eq!(migrated.revision, 2);
    assert_eq!(migrated.config.options["example"]["enabled"], json!(true));
    let chosen = super::super::super::commands::session(&db, ids[1])
        .await
        .unwrap();
    assert_eq!(chosen.revision, 1);
    assert_eq!(chosen.config.options["example"]["enabled"], json!(false));
    assert!(
        super::super::super::commands::session(&db, ids[2])
            .await
            .unwrap()
            .config
            .options
            .is_empty()
    );
    for (id, original) in ids.iter().zip(originals) {
        let old: Vec<u8> = (async {
            let mut statement = db
                .prepare("SELECT config FROM session_revisions WHERE session=?1 AND revision=1")
                .await?;
            let row = statement.query_row([id.to_string()]).await?;
            let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
            mapped
        })
        .await
        .unwrap();
        assert_eq!(old, original);
    }
    assert!(migrate(&db, &info).await.unwrap().is_empty());
    let mut other = info.clone();
    other.summary.name = "other".into();
    assert_eq!(
        check(&db, &other).await.unwrap_err().code,
        ErrorCode::InvalidRequest
    );
}
