use super::*;
use crate::{
    agent::canonical::Role,
    store::agent::tests::fixture::{Fixture, message, request, response},
};
use sailry_protocol::tool::detail::DETAIL_PAGE_BYTES;
use serde_json::json;
use std::sync::Arc;

fn event(branch: &str, name: &str, path: Option<&str>) -> Record {
    let (author, role, part) = match path {
        Some(path) => (
            "assistant",
            Role::Assistant,
            BlockKind::ToolRequest(request("reused", name, json!({"path":path}))),
        ),
        None => (
            "tool",
            Role::User,
            BlockKind::ToolResponse(response("reused", name, json!({"text":"result"}))),
        ),
    };
    let mut event = message(author, role, vec![part]);
    event.provenance.branch = branch.into();
    event
}

#[tokio::test]
async fn single_page_reads_skip_full_leases() {
    let mut fixture = Fixture::new().await;
    let mut result = event("main", "read", None);
    result.message_mut().unwrap().content[0].kind =
        BlockKind::ToolResponse(response("reused", "read", json!({"text":"s".repeat(1024)})));
    let entry = result.id.clone();
    let turn = fixture
        .append([event("main", "read", Some("source.rs")), result])
        .await;
    let db = &fixture.database.connection;
    let source = super::super::super::canonical::event(db, fixture.session, turn, &entry)
        .await
        .unwrap();
    let raw = raw(db, turn, source.sequence, &source.event, 0)
        .await
        .unwrap()
        .unwrap();
    let reference = Reference {
        session: fixture.session,
        turn,
        entry,
        index: 0,
        call: raw.id.clone(),
        revision: raw.revision(),
    };
    let expected = serde_json::to_string(&raw.detail()).unwrap();
    assert!(expected.len() > 1024 && expected.len() < 2048);
    let cache = &fixture.database.tool_details;
    let large: Arc<str> = Arc::from("x".repeat(64 * 1024 * 1024 - 256));
    let leased: Vec<_> = (0..2)
        .map(|index| Reference {
            entry: format!("leased-{index}"),
            ..reference.clone()
        })
        .collect();
    for reference in &leased {
        cache
            .insert_with(reference.clone(), 1, None, Some(DETAIL_PAGE_BYTES), || {
                Ok(large.clone())
            })
            .unwrap();
    }
    for cursor in [None, Some(0)] {
        let page = page(db, cache, reference.clone(), cursor, expected.len())
            .await
            .unwrap();
        assert_eq!(page.text, expected);
        assert_eq!(page.total_bytes, expected.len());
        assert_eq!(page.offset, 0);
        assert_eq!(page.next_offset, None);
        assert!(cache.get(&reference, source.sequence).is_none());
    }
    for (cursor, limit) in [(None, expected.len() - 1), (Some(1), expected.len())] {
        assert_eq!(
            page(db, cache, reference.clone(), cursor, limit)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
    }
    for reference in &leased {
        assert!(Arc::ptr_eq(&cache.get(reference, 1).unwrap().0, &large));
    }
}

#[tokio::test]
async fn preserves_order_branch_and_ambiguity() {
    let mut fixture = Fixture::new().await;
    let turn = fixture
        .append([
            event("main", "read", Some("first")),
            event("main", "read", None),
            event("other", "read", Some("other branch")),
            event("other", "read", None),
            event("main", "write", Some("other name")),
            event("main", "write", None),
            event("main", "read", Some("duplicate")),
            event("main", "read", None),
        ])
        .await;
    let db = &fixture.database.connection;
    let mut statement = db
        .prepare("SELECT sequence,body FROM agent_events WHERE turn=?1 ORDER BY sequence")
        .await
        .unwrap();
    let mut rows = statement.query([turn.to_string()]).await.unwrap();
    let mut events = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        events.push((
            row.get::<i64>(0).unwrap() as u64,
            turn,
            serde_json::from_slice::<Record>(&row.get::<Vec<u8>>(1).unwrap()).unwrap(),
        ));
    }
    let sources = Sources::new(db, &events).await.unwrap();
    let mut expected = [
        json!({"path":"first"}),
        json!({"path":"other branch"}),
        json!({"path":"other name"}),
        Value::Null,
    ]
    .into_iter();
    for (sequence, turn, event) in &events {
        if !matches!(
            event.message().unwrap().content[0].kind,
            BlockKind::ToolResponse(_)
        ) {
            continue;
        }
        let single = raw(db, *turn, *sequence, event, 0).await.unwrap().unwrap();
        let batched = with_sources(db, *turn, *sequence, event, 0, Some(&sources))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(batched.arguments, expected.next().unwrap());
        assert_eq!(
            serde_json::to_value(batched.detail()).unwrap(),
            serde_json::to_value(single.detail()).unwrap()
        );
        assert_eq!(batched.source, single.source);
    }
    assert!(expected.next().is_none());
    // A continuation containing results alone still resolves earlier calls once.
    let results: Vec<_> = events
        .into_iter()
        .filter(|(_, _, event)| {
            matches!(
                event.message().unwrap().content[0].kind,
                BlockKind::ToolResponse(_)
            )
        })
        .collect();
    let sources = Sources::new(db, &results).await.unwrap();
    let (sequence, turn, event) = &results[0];
    assert_eq!(
        with_sources(db, *turn, *sequence, event, 0, Some(&sources))
            .await
            .unwrap()
            .unwrap()
            .arguments,
        json!({"path":"first"})
    );
}
