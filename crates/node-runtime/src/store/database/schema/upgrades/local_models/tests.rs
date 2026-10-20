use super::*;
use sailry_protocol::Command;
use serde_json::json;
#[test]
fn converts_captured_commands() {
    use crate::store::agent::approvals::Prepared;
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE local_models(id TEXT PRIMARY KEY,kind TEXT NOT NULL,digest TEXT NOT NULL UNIQUE,body BLOB NOT NULL);
        CREATE TABLE local_model_settings(singleton INTEGER PRIMARY KEY,body BLOB NOT NULL);
        CREATE TABLE agent_approvals(prepared BLOB NOT NULL);").unwrap();
    for (old, new) in [
        (
            "install_ocr_model",
            Command::InstallLocalModel { id: "a".repeat(64) },
        ),
        (
            "delete_ocr_model",
            Command::DeleteLocalModel { id: "a".repeat(64) },
        ),
    ] {
        let mut command = serde_json::to_value(&new).unwrap();
        command["kind"] = json!(old);
        db.execute(
            "INSERT INTO agent_approvals(prepared) VALUES(?1)",
            [encode(&json!({"command":command,"digest":"original","targets":[]})).unwrap()],
        )
        .unwrap();
    }
    assert!(reconcile(&db).unwrap());
    for (row, command) in [
        (1, Command::InstallLocalModel { id: "a".repeat(64) }),
        (2, Command::DeleteLocalModel { id: "a".repeat(64) }),
    ] {
        let body: Vec<u8> = db
            .query_row(
                "SELECT prepared FROM agent_approvals WHERE rowid=?1",
                [row],
                |row| row.get(0),
            )
            .unwrap();
        let prepared: Prepared = serde_json::from_slice(&body).unwrap();
        assert!(prepared.matches(&command).unwrap());
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["command"]["data"]["id"], "a".repeat(64));
        assert_eq!(value["targets"], json!([]));
    }
    assert!(!reconcile(&db).unwrap());
}
