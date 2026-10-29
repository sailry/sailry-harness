use super::*;
use crate::agent::canonical::{self, BlockKind, Record, Role, ToolStatus};
use crate::store::agent::tests::fixture::{message, request, response};
use serde_json::json;

#[test]
fn captures_status_without_reading_payloads() {
    let timeline = crate::agent::observation::Timeline::new();
    for (id, code) in [
        ("cancelled", sailry_protocol::ErrorCode::Cancelled),
        ("unknown", sailry_protocol::ErrorCode::OutcomeUnknown),
    ] {
        timeline.fail_tool("", id, "same", Some(code));
    }
    let blocks = [
        ("business", ToolStatus::Success),
        ("failed", ToolStatus::Error),
        ("cancelled", ToolStatus::Error),
        ("unknown", ToolStatus::Error),
    ]
    .into_iter()
    .map(|(id, status)| {
        let mut result = response(
            id,
            "same",
            json!({"isError":true,"error":"unchanged business data",
            "status":"failed"}),
        );
        result.status = status;
        BlockKind::ToolResponse(result)
    })
    .collect();
    let mut event = message("assistant", Role::User, blocks);
    let original = serde_json::to_value(&event.message().unwrap().content).unwrap();
    capture_failures(&mut event, Some(&timeline));
    let captured = failures(&event).unwrap();
    assert_eq!(captured.len(), 3);
    assert_eq!(captured[0].id, "failed");
    assert_eq!(captured[0].failure.code, None);
    assert_eq!(captured[1].id, "cancelled");
    assert_eq!(
        captured[1].failure.code,
        Some(sailry_protocol::ErrorCode::Cancelled)
    );
    assert_eq!(captured[2].id, "unknown");
    assert_eq!(
        captured[2].failure.code,
        Some(sailry_protocol::ErrorCode::OutcomeUnknown)
    );
    assert_eq!(
        serde_json::to_value(&event.message().unwrap().content).unwrap(),
        original
    );
}

#[test]
fn matches_only_observed_failed_calls() {
    let timeline = crate::agent::observation::Timeline::new();
    timeline.fail_tool(
        "branch",
        "failed",
        "same",
        Some(sailry_protocol::ErrorCode::Cancelled),
    );
    let mut event = message(
        "assistant",
        Role::User,
        ["business", "failed"]
            .into_iter()
            .map(|id| {
                BlockKind::ToolResponse(response(id, "same", json!({"error":"unchanged data"})))
            })
            .collect(),
    );
    event.provenance.branch = "branch".into();
    let original = serde_json::to_value(&event.message().unwrap().content).unwrap();
    capture_failures(&mut event, Some(&timeline));
    let observed = failures(&event).unwrap();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].index, 1);
    assert_eq!(observed[0].id, "failed");
    assert_eq!(
        observed[0].failure.code,
        Some(sailry_protocol::ErrorCode::Cancelled)
    );
    assert_eq!(
        serde_json::to_value(&event.message().unwrap().content).unwrap(),
        original
    );
    let saved = serde_json::to_vec(&event).unwrap();
    timeline.forget_failure("branch", "failed", "same");
    capture_failures(&mut event, Some(&timeline));
    assert!(failures(&event).unwrap().is_empty());
    let mut restored = event;
    preserve_failures(&mut restored, Some(&saved)).unwrap();
    assert_eq!(failures(&restored).unwrap().len(), 1);
}

#[test]
fn preserves_only_matching_response_facts() {
    let timeline = crate::agent::observation::Timeline::new();
    timeline.fail_tool("branch", "failed", "same", None);
    let mut event = message(
        "assistant",
        Role::User,
        vec![BlockKind::ToolResponse(response(
            "failed",
            "same",
            json!({"error":"unchanged data"}),
        ))],
    );
    event.provenance.branch = "branch".into();
    capture_failures(&mut event, Some(&timeline));
    let saved = serde_json::to_vec(&event).unwrap();
    for mismatch in ["branch", "id", "name", "result"] {
        let mut current = event.clone();
        current
            .metadata
            .presentation
            .as_mut()
            .unwrap()
            .failures
            .clear();
        if mismatch == "branch" {
            current.provenance.branch = "other".into();
        } else {
            let BlockKind::ToolResponse(result) =
                &mut current.message_mut().unwrap().content[0].kind
            else {
                unreachable!()
            };
            match mismatch {
                "id" => result.id = "other".into(),
                "name" => result.name = "other".into(),
                "result" => {
                    *result = response(&result.id, &result.name, json!({"error":"changed data"}))
                }
                _ => unreachable!(),
            }
        }
        preserve_failures(&mut current, Some(&saved)).unwrap();
        assert!(failures(&current).unwrap().is_empty(), "{mismatch}");
    }
}

#[tokio::test]
async fn freezes_owned_session_names() {
    use crate::store::agent::tests::fixture::Fixture;
    let mut fixture = Fixture::new().await;
    let turn = fixture.queued().await;
    let parent = super::super::runs::get(&fixture.database.connection, turn)
        .await
        .unwrap();
    let mut content = Vec::new();
    let mut expected = BTreeMap::new();
    for title in ["A", "B", "C"] {
        let mut child = parent.clone();
        child.turn = sailry_protocol::TurnId::new();
        child.session = sailry_protocol::SessionId::new();
        super::super::background::child_started(
            &fixture.database.connection,
            &parent,
            &child,
            title,
        )
        .await
        .unwrap();
        expected.insert(child.session.to_string(), title.to_owned());
        content.push(BlockKind::ToolRequest(request(
            &format!("wait-{title}"),
            "observe",
            json!({"session":child.session}),
        )));
    }
    let mut event = message("assistant", Role::Assistant, content);
    let display = serde_json::from_value(serde_json::json!({"label":"Observe", "input":{"summary":{"path":"/session","format":"session"}}})).unwrap();
    stamp(
        &mut event,
        &canonical::Presentation {
            displays: BTreeMap::from([("observe".into(), display)]),
            ..Default::default()
        },
    );
    let original = event.clone();
    capture_names(
        &fixture.database.connection,
        fixture.session,
        &mut event,
        None,
    )
    .await
    .unwrap();
    let captured = displays(&event).unwrap();
    let display = &captured["observe"];
    assert!(display.valid());
    assert_eq!(
        display
            .input
            .as_ref()
            .unwrap()
            .summary
            .as_ref()
            .unwrap()
            .names,
        expected
    );
    let persisted = serde_json::to_vec(&event).unwrap();
    fixture
        .database
        .connection
        .execute(
            "UPDATE agent_background SET body=json_set(body,'$.task.title','Changed')",
            (),
        )
        .await
        .unwrap();
    let mut repeated = original.clone();
    capture_names(
        &fixture.database.connection,
        fixture.session,
        &mut repeated,
        Some(&persisted),
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::to_value(&repeated).unwrap(),
        serde_json::to_value(&event).unwrap()
    );
    let mut foreign = original;
    capture_names(
        &fixture.database.connection,
        sailry_protocol::SessionId::new(),
        &mut foreign,
        None,
    )
    .await
    .unwrap();
    assert!(
        displays(&foreign).unwrap()["observe"]
            .input
            .as_ref()
            .unwrap()
            .summary
            .as_ref()
            .unwrap()
            .names
            .is_empty()
    );
    assert_eq!(
        serde_json::to_value(repeated.message().map(|message| &message.content)).unwrap(),
        serde_json::to_value(event.message().map(|message| &message.content)).unwrap()
    );
}

#[test]
fn captures_trusted_command_aliases() {
    use sailry_protocol::tool::{Alias, AliasKind};
    let arguments = json!({"command":"/node/home/python/bin/python /node/home/task.py"});
    let mut event = message(
        "assistant",
        Role::Assistant,
        vec![BlockKind::ToolRequest(request(
            "call",
            "probe",
            arguments.clone(),
        ))],
    );
    event.metadata.presentation = Some(canonical::Presentation {
        displays: serde_json::from_value(
            json!({"probe":{"label":"Forged","aliases":[{"path":"/node","kind":"python"}]}}),
        )
        .unwrap(),
        ..Default::default()
    });
    let mut display: sailry_protocol::tool::Display = serde_json::from_value(
        json!({"label":"Run","input":{"summary":{"path":"/command","format":"command"}}}),
    )
    .unwrap();
    let aliases = vec![
        Alias {
            path: "/node/home".into(),
            kind: AliasKind::Home,
        },
        Alias {
            path: "/node/home/python/bin/python".into(),
            kind: AliasKind::Python,
        },
    ];
    display.aliases = aliases.clone();
    stamp(
        &mut event,
        &canonical::Presentation {
            displays: BTreeMap::from([("probe".into(), display)]),
            ..Default::default()
        },
    );
    let restored: Record = serde_json::from_slice(&serde_json::to_vec(&event).unwrap()).unwrap();
    assert_eq!(displays(&restored).unwrap()["probe"].aliases, aliases);
    let BlockKind::ToolRequest(call) = &restored.message().unwrap().content[0].kind else {
        panic!("call expected")
    };
    assert_eq!(call.arguments, arguments);
}

#[test]
fn captures_package_display_and_rejects_provider_metadata() {
    let display = serde_json::from_value(json!({
        "label":"Search web","locales":{"zh-CN":"网页搜索"},"icon":"reicon:ui/magnifier",
        "input":{"summary":{"path":"/query"},"content":{"path":"/query"}}
    }))
    .unwrap();
    let displays = BTreeMap::from([("web_search".into(), display)]);
    let block = BlockKind::ToolRequest(request("call", "web_search", json!({})));
    let mut event = message("assistant", Role::Assistant, vec![block]);
    event.metadata.presentation = Some(canonical::Presentation {
        displays: serde_json::from_value(json!({"web_search":{"label":"Forged"}})).unwrap(),
        ..Default::default()
    });
    stamp(
        &mut event,
        &canonical::Presentation {
            displays: displays.clone(),
            ..Default::default()
        },
    );
    assert_eq!(super::displays(&event).unwrap(), displays);
    let restored: Record = serde_json::from_slice(&serde_json::to_vec(&event).unwrap()).unwrap();
    assert_eq!(super::displays(&restored).unwrap(), displays);
    stamp(&mut event, &canonical::Presentation::default());
    assert!(super::displays(&event).unwrap().is_empty());
}

#[test]
fn captures_registered_calls() {
    let mut event = message(
        "assistant",
        Role::Assistant,
        vec![BlockKind::ToolRequest(request(
            "call",
            "probe",
            json!({"presentation":"details","grouping":"sequence"}),
        ))],
    );
    event.metadata.presentation = Some(canonical::Presentation {
        tools: BTreeMap::from([
            ("other".into(), Presentation::Summary),
            ("probe".into(), Presentation::Details),
        ]),
        groupings: BTreeMap::from([("probe".into(), sailry_protocol::tool::Grouping::Sequence)]),
        ..Default::default()
    });
    let catalog = BTreeMap::from([
        ("probe".into(), Presentation::Summary),
        ("unused".into(), Presentation::Details),
    ]);
    let grouping = BTreeMap::from([("probe".into(), sailry_protocol::tool::Grouping::Standalone)]);
    stamp(
        &mut event,
        &canonical::Presentation {
            tools: catalog,
            groupings: grouping.clone(),
            ..Default::default()
        },
    );
    assert_eq!(groupings(&event).unwrap(), grouping);
    assert_eq!(
        read(&event).unwrap(),
        BTreeMap::from([("probe".into(), Presentation::Summary)])
    );
    let restored: Record = serde_json::from_slice(&serde_json::to_vec(&event).unwrap()).unwrap();
    assert_eq!(read(&restored).unwrap(), read(&event).unwrap());
    assert_eq!(groupings(&restored).unwrap(), grouping);
    stamp(&mut event, &canonical::Presentation::default());
    assert!(read(&event).unwrap().is_empty());
    assert!(groupings(&event).unwrap().is_empty());
}
