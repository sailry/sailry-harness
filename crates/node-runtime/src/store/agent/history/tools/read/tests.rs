use super::*;
use crate::store::agent::tests::fixture::Fixture;
use serde_json::json;

fn event(branch: &str, name: &str, path: Option<&str>) -> AdkEvent {
    let mut event = AdkEvent::new("source-fixture");
    event.branch = branch.into();
    event.author = if path.is_some() { "assistant" } else { "tool" }.into();
    let part = match path {
        Some(path) => adk_core::Part::FunctionCall {
            id: Some("reused".into()),
            name: name.into(),
            args: json!({"path":path}),
            thought_signature: None,
        },
        None => adk_core::Part::FunctionResponse {
            id: Some("reused".into()),
            function_response: adk_core::FunctionResponseData::new(name, json!({"text":"result"})),
            annotations: None,
        },
    };
    event.set_content(adk_core::Content {
        role: if path.is_some() { "model" } else { "function" }.into(),
        parts: vec![part],
    });
    event
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
            serde_json::from_slice::<AdkEvent>(&row.get::<Vec<u8>>(1).unwrap()).unwrap(),
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
            event.content().unwrap().parts[0],
            adk_core::Part::FunctionResponse { .. }
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
                event.content().unwrap().parts[0],
                adk_core::Part::FunctionResponse { .. }
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

#[tokio::test]
async fn skips_unrequested_call_projection() {
    let mut fixture = Fixture::new().await;
    let mut unrelated = [
        event("main", "other", Some("other name")),
        event("main", "read", Some("other id")),
    ];
    let mut content = unrelated[1].content().unwrap().clone();
    if let adk_core::Part::FunctionCall { id, .. } = &mut content.parts[0] {
        *id = Some("unrequested".into());
    }
    unrelated[1].set_content(content);
    let result = event("main", "read", None);
    let turn = fixture
        .append(
            unrelated
                .clone()
                .into_iter()
                .chain([event("main", "read", Some("requested")), result.clone()]),
        )
        .await;
    let db = &fixture.database.connection;
    // Invalid display metadata on an unrelated call must never be evaluated.
    for mut call in unrelated {
        call.provider_metadata
            .insert("dev.sailry.tool_display".into(), "[]".into());
        assert_eq!(
            db.execute(
                "UPDATE agent_events SET body=?1 WHERE turn=?2 AND json_extract(body,'$.id')=?3",
                params![
                    serde_json::to_vec(&call).unwrap(),
                    turn.to_string(),
                    call.id
                ],
            )
            .await
            .unwrap(),
            1
        );
    }
    let sequence = db
        .prepare("SELECT sequence FROM agent_events WHERE turn=?1 AND json_extract(body,'$.id')=?2")
        .await
        .unwrap()
        .query_row(params![turn.to_string(), result.id.clone()])
        .await
        .unwrap()
        .get::<i64>(0)
        .unwrap() as u64;
    let sources = Sources::new(db, &[(sequence, turn, result.clone())])
        .await
        .unwrap();
    assert_eq!(
        with_sources(db, turn, sequence, &result, 0, Some(&sources))
            .await
            .unwrap()
            .unwrap()
            .arguments,
        json!({"path":"requested"})
    );
}
