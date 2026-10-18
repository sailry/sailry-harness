use super::*;
use sailry_protocol::{ProviderId, SessionId, WorktreeId};
use serde_json::json;
use std::collections::BTreeMap;

fn config() -> SessionConfig {
    serde_json::from_value(json!({"provider":ProviderId::new(),"model":"fixture","mode":"code","permission":"ask","credential":null})).unwrap()
}

fn fixture() -> (Connection, Info) {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../../schema.sql")).unwrap();
    let info: Info=serde_json::from_value(json!({
        "summary":{"name":"example","revision":1,"settings_revision":0,"enabled":true,"digest":"a".repeat(64)},
        "extension":{"api_version":"v1","conversation_options":{"$schema":plugin::settings::SCHEMA,"type":"object","additionalProperties":false,
            "properties":{"enabled":{"type":"boolean","default":false,"x-sailry-import":"prior_flag"}},"required":["enabled"]}}
    })).unwrap();
    super::super::save(&db, &info, true).unwrap();
    (db, info)
}

#[test]
fn confines_changes_to_declared_owner() {
    let (db, info) = fixture();
    let before = config();
    let mut after = before.clone();
    after.options.insert(
        "example".into(),
        BTreeMap::from([("enabled".into(), json!(true))]),
    );
    validate(&db, Some(&before), &after, Some(&info.summary.reference())).unwrap();
    let mut foreign = info.summary.reference();
    foreign.name = "other".into();
    assert_eq!(
        validate(&db, Some(&before), &after, Some(&foreign))
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    let mut stale = info.summary.reference();
    stale.settings_revision += 1;
    assert_eq!(
        validate(&db, Some(&before), &after, Some(&stale))
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
        validate(&db, Some(&before), &after, None).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    let mut retained = before.clone();
    retained.options.insert(
        "removed-package".into(),
        BTreeMap::from([("choice".into(), json!(true))]),
    );
    let mut changed = retained.clone();
    changed.mode = sailry_protocol::WorkMode::Plan;
    validate(&db, Some(&retained), &changed, None).unwrap();
}

#[test]
fn imports_only_current_explicit_preferences() {
    let (db, info) = fixture();
    let mut ids = vec![];
    let mut originals = vec![];
    for (flag, chosen) in [(true, None), (true, Some(false)), (false, None)] {
        let id = SessionId::new();
        let worktree = WorktreeId::new();
        db.execute(
            "INSERT INTO worktrees(id,path,main) VALUES(?1,?2,0)",
            params![worktree.to_string(), format!("/fixture/{worktree}")],
        )
        .unwrap();
        let mut raw = serde_json::to_value(config()).unwrap();
        raw["prior_flag"] = json!(flag);
        if let Some(chosen) = chosen {
            raw["options"] = json!({"example":{"enabled":chosen}});
        }
        let bytes = encode(&raw).unwrap();
        db.execute(
            "INSERT INTO sessions(id,worktree,revision) VALUES(?1,?2,1)",
            params![id.to_string(), worktree.to_string()],
        )
        .unwrap();
        db.execute("INSERT INTO session_revisions(session,revision,worktree,config,roles) VALUES(?1,1,?2,?3,?4)",params![id.to_string(),worktree.to_string(),bytes,encode(&sailry_protocol::role::Snapshot::default()).unwrap()]).unwrap();
        ids.push(id);
        originals.push(bytes);
    }
    // This historical value must not leak into a newer configuration lacking the source.
    let mut current = super::super::super::commands::session(&db, ids[2]).unwrap();
    current.revision = 2;
    db.execute(
        "UPDATE sessions SET revision=2 WHERE id=?1",
        [ids[2].to_string()],
    )
    .unwrap();
    db.execute("INSERT INTO session_revisions(session,revision,worktree,config,roles) VALUES(?1,2,?2,?3,?4)",params![current.id.to_string(),current.worktree.to_string(),encode(&config()).unwrap(),encode(&current.roles).unwrap()]).unwrap();
    let changes = migrate(&db, &info).unwrap();
    assert_eq!(changes.len(), 1);
    let migrated = super::super::super::commands::session(&db, ids[0]).unwrap();
    assert_eq!(migrated.revision, 2);
    assert_eq!(migrated.config.options["example"]["enabled"], json!(true));
    let chosen = super::super::super::commands::session(&db, ids[1]).unwrap();
    assert_eq!(chosen.revision, 1);
    assert_eq!(chosen.config.options["example"]["enabled"], json!(false));
    assert!(
        super::super::super::commands::session(&db, ids[2])
            .unwrap()
            .config
            .options
            .is_empty()
    );
    for (id, original) in ids.iter().zip(originals) {
        let old: Vec<u8> = db
            .query_row(
                "SELECT config FROM session_revisions WHERE session=?1 AND revision=1",
                [id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(old, original);
    }
    assert!(migrate(&db, &info).unwrap().is_empty());
    let mut other = info.clone();
    other.summary.name = "other".into();
    assert_eq!(
        check(&db, &other).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
}
