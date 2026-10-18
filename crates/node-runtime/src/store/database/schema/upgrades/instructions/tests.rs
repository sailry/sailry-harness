use super::*;
use sailry_protocol::{Command, NodeId, ProviderId, Request, SessionId, TurnId, WorktreeId};
use serde_json::json;

fn request(db: &Connection) -> Request {
    let request = Request::new(NodeId([7; 32]), Command::ReadNetworkSettings);
    db.execute(
        "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'completed')",
        params![
            request.target.0.as_slice(),
            request.id.to_string(),
            serde_json::to_vec(&request).unwrap()
        ],
    )
    .unwrap();
    request
}

#[test]
fn preserves_exact_resources_and_raw_configuration() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../../../../schema.sql"))
        .unwrap();
    let info: plugin::Info=serde_json::from_value(json!({
        "summary":{"name":"neutral-worker","revision":1,"settings_revision":0,"enabled":true,"digest":"a".repeat(64)},
        "extension":{"api_version":"v1","actions":["agents.delegate"],"tools":[{"name":"worker","operation":"agents.delegate","description":"Independent work"}]}
    })).unwrap();
    let package = serde_json::to_vec(&info).unwrap();
    db.execute(
        "INSERT INTO plugin_packages(digest,name,body) VALUES(?1,?2,?3)",
        params![info.summary.digest, info.summary.name, package],
    )
    .unwrap();
    let references = serde_json::to_vec(&vec![info.summary.reference()]).unwrap();
    let roles = serde_json::to_vec(&sailry_protocol::role::Snapshot::default()).unwrap();
    let mut originals = vec![];
    for flag in [true, false] {
        let session = SessionId::new();
        let turn = TurnId::new();
        let worktree = WorktreeId::new();
        db.execute(
            "INSERT INTO worktrees(id,path,main) VALUES(?1,?2,0)",
            params![worktree.to_string(), format!("/fixture/{worktree}")],
        )
        .unwrap();
        db.execute(
            "INSERT INTO sessions(id,worktree,revision) VALUES(?1,?2,1)",
            params![session.to_string(), worktree.to_string()],
        )
        .unwrap();
        let raw=serde_json::to_vec(&json!({"provider":ProviderId::new(),"model":"fixture","mode":"code","permission":"ask","credential":null,"delegation_proactive":flag})).unwrap();
        db.execute("INSERT INTO session_revisions(session,revision,worktree,config,roles) VALUES(?1,1,?2,?3,?4)",params![session.to_string(),worktree.to_string(),raw,roles]).unwrap();
        let request = request(&db);
        db.execute("INSERT INTO turns(id,kind,session,revision,request,caller,plugins,roles) VALUES(?1,'task',?2,1,?3,?4,?5,?6)",params![turn.to_string(),session.to_string(),request.id.to_string(),request.target.0.as_slice(),references,roles]).unwrap();
        originals.push((session, turn, raw, flag));
    }
    assert!(reconcile(&db).unwrap());
    assert!(!reconcile(&db).unwrap());
    let later = TurnId::new();
    let request = request(&db);
    db.execute("INSERT INTO turns(id,kind,session,revision,request,caller,plugins,roles) VALUES(?1,'task',?2,1,?3,?4,?5,?6)",params![later.to_string(),originals[0].0.to_string(),request.id.to_string(),request.target.0.as_slice(),references,roles]).unwrap();
    let plan =
        super::super::super::plan::Plan::prepare(&db, std::path::Path::new("fixture")).unwrap();
    assert!(!plan.instructions);
    super::super::reconcile(&db, plan.instructions).unwrap();
    assert!(
        crate::store::captures::instructions(&db, later)
            .unwrap()
            .is_empty()
    );
    for (session, turn, raw, flag) in originals {
        let unchanged: Vec<u8> = db
            .query_row(
                "SELECT config FROM session_revisions WHERE session=?1",
                [session.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unchanged, raw);
        let (plugins, stored_roles): (Vec<u8>, Vec<u8>) = db
            .query_row(
                "SELECT plugins,roles FROM turns WHERE id=?1",
                [turn.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(plugins, references);
        assert_eq!(stored_roles, roles);
        let captured = crate::store::captures::instructions(&db, turn).unwrap();
        assert_eq!(captured.len(), usize::from(flag));
        if flag {
            assert_eq!(captured[0].text, RETAINED);
            assert_eq!(
                captured[0].any_tools,
                vec![crate::plugins::tools::alias("neutral-worker", "worker")]
            );
        }
    }
    let retained: Vec<u8> = db
        .query_row(
            "SELECT body FROM plugin_packages WHERE digest=?1",
            [&info.summary.digest],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(retained, package);
}
