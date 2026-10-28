//! Opt-in evidence uses a closed private snapshot, never the running source profile.
use super::*;
use crate::agent::canonical::{BlockKind, Record};
use serde_json::Value;

async fn preserved(db: &Connection) -> [blake3::Hash; 3] {
    let mut hashes = Vec::new();
    for query in [
        "SELECT json_group_array(json_object('id',id,'body',hex(body))) FROM (SELECT * FROM providers ORDER BY id)",
        "SELECT json_group_array(json_object('session',session,'revision',revision,'worktree',worktree,'config',hex(config),'profile',hex(profile),'roles',hex(roles))) FROM (SELECT * FROM session_revisions ORDER BY session,revision)",
        "SELECT json_group_array(json_object('id',id,'kind',kind,'plugins',hex(plugins),'roles',hex(roles),'instructions',hex(instructions),'session',session,'revision',revision,'request',request,'caller',hex(caller))) FROM (SELECT * FROM turns ORDER BY id)",
    ] {
        let value: String = db
            .prepare(query)
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get(0)
            .unwrap();
        hashes.push(blake3::hash(value.as_bytes()));
    }
    hashes.try_into().unwrap()
}

#[tokio::test]
#[ignore = "Requires SAILRY_ACCEPTANCE_SNAPSHOT pointing to a closed private Sailry backup"]
async fn upgrades_private_snapshot() {
    let source = PathBuf::from(
        std::env::var_os("SAILRY_ACCEPTANCE_SNAPSHOT").expect("private snapshot required"),
    );
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("node.turso");
    schema::backup::copy(&source, &path).unwrap();
    let (node, before, originals, nullable) = {
        let handle = engine(&path).await.unwrap();
        let connection = connect(&handle).await.unwrap();
        let node = NodeId(
            connection
                .prepare("SELECT identity FROM node WHERE singleton=1")
                .await
                .unwrap()
                .query_row(())
                .await
                .unwrap()
                .get::<[u8; 32]>(0)
                .unwrap(),
        );
        let before = preserved(&connection).await;
        let mut rows = connection
            .query(
                "SELECT sequence,turn,body FROM agent_events ORDER BY sequence",
                (),
            )
            .await
            .unwrap();
        let mut originals = Vec::new();
        let mut nullable = 0;
        while let Some(row) = rows.next().await.unwrap() {
            let sequence = row.get::<i64>(0).unwrap();
            let turn = row.get::<String>(1).unwrap();
            let event: Value = serde_json::from_slice(&row.get::<Vec<u8>>(2).unwrap())
                .unwrap_or_else(|_| {
                    panic!("private snapshot event cannot decode at sequence {sequence}")
                });
            nullable += event["content"]["parts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|part| {
                    part.get("name").is_some() && part.get("args").is_some() && part["id"].is_null()
                })
                .count();
            originals.push((sequence, turn, event));
        }
        drop(rows);
        (node, before, originals, nullable)
    };
    assert!(!originals.is_empty());
    // upgrade() has no recovery or Agent supervisor and cannot claim historical tasks.
    let (_handle, connection, backup) = upgrade(&path, node, None).await.unwrap();
    assert!(backup.is_some());
    assert_eq!(preserved(&connection).await, before);
    for (sequence, turn, event) in &originals {
        let body = connection
            .prepare("SELECT body FROM agent_events WHERE sequence=?1")
            .await
            .unwrap()
            .query_row([*sequence])
            .await
            .unwrap()
            .get::<Vec<u8>>(0)
            .unwrap();
        let record: Record = serde_json::from_slice(&body).unwrap_or_else(|_| {
            panic!("private snapshot record cannot decode at sequence {sequence}")
        });
        assert!(
            Some(record.id.as_str()) == event["id"].as_str()
                && record.turn.to_string() == *turn
                && Some(record.created)
                    == event["timestamp"]
                        .as_str()
                        .and_then(|value| value.parse::<chrono::DateTime<chrono::Utc>>().ok())
                && Some(record.provenance.author.as_str()) == event["author"].as_str()
                && Some(record.provenance.branch.as_str()) == event["branch"].as_str(),
            "record provenance changed at sequence {sequence}"
        );
        for (index, part) in event["content"]["parts"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let block = record
                .message()
                .and_then(|message| message.content.iter().find(|block| block.index == index));
            if part.get("name").is_some() && part.get("args").is_some() {
                assert!(
                    matches!(block.map(|block|&block.kind),Some(BlockKind::ToolRequest(request))
                    if request.id.as_deref()==part["id"].as_str() && Some(request.name.as_str())==part["name"].as_str()
                        && request.arguments==part["args"] && request.signature.as_deref()==part["thought_signature"].as_str()),
                    "tool call fields changed at sequence {sequence}"
                );
            } else if let Some(raw) = part.get("functionResponse") {
                let Some(BlockKind::ToolResponse(response)) = block.map(|block| &block.kind) else {
                    panic!("tool result missing at sequence {sequence}")
                };
                assert!(
                    response.id.as_deref() == part["id"].as_str()
                        && Some(response.name.as_str()) == raw["name"].as_str(),
                    "tool result identity changed at sequence {sequence}"
                );
                let value = crate::store::agent::blobs::hydrate(&connection, &response.result)
                    .await
                    .unwrap();
                assert!(
                    value == raw["response"],
                    "tool result changed at sequence {sequence}"
                );
                assert!(
                    response.revision
                        == blake3::hash(raw["response"].to_string().as_bytes())
                            .to_hex()
                            .as_str(),
                    "tool result revision changed at sequence {sequence}"
                );
            }
        }
    }

    let blobs: i64 = connection
        .prepare("SELECT count(*) FROM agent_blobs")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get(0)
        .unwrap();
    println!(
        "private snapshot: records={}, blobs={blobs}, nullable_call_ids={nullable}, captures=retained, provider_configuration=retained",
        originals.len()
    );
}
