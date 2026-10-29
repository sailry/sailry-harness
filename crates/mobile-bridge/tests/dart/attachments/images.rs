use super::super::{agent_support, database_fixture, tools};
use base64::Engine;
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{Command, Output, SessionId, TurnId, conversation::Status};
use serde_json::{Value, json};
use std::path::Path;

pub(super) fn contract() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(agent_support::Server::tools(vec![]));
    let (node, sessions) = runtime.block_on(prepare(directory.path(), &server.endpoint));
    assert_eq!(server.requests.lock().unwrap().len(), 1);
    let invitation = node.link().invite().unwrap();
    let child = super::super::command("images.dart", directory.path(), invitation.ticket())
        .env("SAILRY_PROJECT_PATH", root)
        .env("SAILRY_SESSIONS", serde_json::to_string(&sessions).unwrap())
        .spawn()
        .unwrap();
    let status = super::super::wait(child);
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

async fn prepare(directory: &Path, endpoint: &str) -> (Node, Vec<SessionId>) {
    let (node, sessions) = tools::sessions(directory, endpoint, "ffi-image").await;
    let client = Client::new(node.local());
    let Output::QueuedTurn(turn) = client
        .execute(client.prepare(Command::SubmitTurn {
            session: sessions[0],
            expected_revision: 1,
            message: "Return the fixture response".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let page = client
                .read_conversation(sessions[0], None, 100)
                .await
                .unwrap()
                .page;
            if page
                .runs
                .iter()
                .any(|run| run.turn == turn.id && run.status == Status::Completed)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let profile = node.profile().to_owned();
    drop(client);
    node.shutdown().await.unwrap();
    seed(&profile, turn.id).await;
    (Node::start(&profile).await.unwrap(), sessions)
}

async fn seed(profile: &Path, turn: TurnId) {
    // As in the resource image fixture, seed an isolated, stopped canonical
    // response. Native provider image decoding is outside this FFI contract.
    let bytes = base64::engine::general_purpose::STANDARD.decode(
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j3ioAAAAASUVORK5CYII="
    ).unwrap();
    let digest = blake3::hash(&bytes).to_hex().to_string();
    let (handle, mut db) = database_fixture::open(&profile.join("storage/node.turso"), false).await;
    let transaction = db.transaction().await.unwrap();
    transaction
        .execute(
            "INSERT INTO agent_blobs(digest,bytes) VALUES(?1,?2)",
            turso::params![digest.clone(), bytes.clone()],
        )
        .await
        .unwrap();
    let records = {
        let mut rows = transaction
            .query(
                "SELECT sequence,body FROM agent_events WHERE turn=?1 ORDER BY sequence",
                [turn.to_string()],
            )
            .await
            .unwrap();
        let mut records = Vec::new();
        while let Some(row) = rows.next().await.unwrap() {
            records.push((row.get::<i64>(0).unwrap(), row.get::<Vec<u8>>(1).unwrap()));
        }
        records
    };
    let mut seeded = false;
    for (sequence, body) in records {
        let mut record: Value = serde_json::from_slice(&body).unwrap();
        if record["kind"] != "message"
            || record["role"] != "assistant"
            || !record["content"]
                .as_array()
                .is_some_and(|blocks| blocks.iter().any(|block| block["type"] == "text"))
        {
            continue;
        }
        record["part_count"] = json!(3);
        record["content"] = json!([
            {"index":0,"type":"text","text":"Before the image"},
            {"index":1,"type":"image","asset":{"digest":digest,"mime":"image/png","size":bytes.len()}},
            {"index":2,"type":"text","text":"After the image"}
        ]);
        transaction
            .execute(
                "UPDATE agent_events SET body=?2 WHERE sequence=?1",
                turso::params![sequence, serde_json::to_vec(&record).unwrap()],
            )
            .await
            .unwrap();
        seeded = true;
        break;
    }
    assert!(seeded, "canonical assistant text response expected");
    transaction.commit().await.unwrap();
    drop(db);
    drop(handle);
}
