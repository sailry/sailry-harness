use super::super::compaction as context;
use super::{fixture::Fixture, *};

fn event(text: &str) -> AdkEvent {
    let mut event = AdkEvent::new("context-fixture");
    event.author = "assistant".into();
    event.set_content(adk_core::Content::new("model").with_text(text));
    event
}

fn summary(end: &AdkEvent) -> AdkEvent {
    let mut summary = AdkEvent::new("summary-fixture");
    summary.author = "system".into();
    summary
        .provider_metadata
        .insert(context::END_EVENT.into(), end.id.clone());
    summary.actions.compaction = Some(adk_core::EventCompaction {
        start_timestamp: end.timestamp,
        end_timestamp: end.timestamp,
        compacted_content: adk_core::Content::new("user").with_text("Summary 中文 🙂"),
    });
    summary
}

async fn load(fixture: &Fixture, turn: TurnId) -> Vec<AdkEvent> {
    sessions::get(
        &fixture.database.connection,
        turn,
        GetRequest {
            app_name: APP.into(),
            user_id: USER.into(),
            session_id: fixture.session.to_string(),
            num_recent_events: None,
            after: None,
        },
    )
    .await
    .unwrap()
    .events()
    .all()
}

#[tokio::test]
async fn retains_summary_only_update_time() {
    let mut fixture = Fixture::new().await;
    let original = event("Original evidence");
    fixture.append([original.clone()]).await;
    let compacted = summary(&original);
    let turn = fixture.append([compacted.clone()]).await;
    let session = sessions::get(
        &fixture.database.connection,
        turn,
        GetRequest {
            app_name: APP.into(),
            user_id: USER.into(),
            session_id: fixture.session.to_string(),
            num_recent_events: None,
            after: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(session.events().len(), 1);
    assert_eq!(session.last_update_time(), compacted.timestamp);
}

#[tokio::test]
async fn materializes_only_canonical_owner_sources() {
    use adk_core::{Content, FunctionResponseData, Part as AdkPart};
    use serde_json::json;

    let mut fixture = Fixture::new().await;
    let mut captured = AdkEvent::new("captured-owners");
    captured.author = "system".into();
    captured.actions.state_delta.extend([
        (
            "sailry.context.policy".into(),
            json!("Exact captured policy"),
        ),
        (
            "sailry.context.environment".into(),
            json!("Exact captured environment"),
        ),
        (
            "sailry.context.masked_outputs".into(),
            json!([["retained-call", "captured-revision"]]),
        ),
        (
            "sailry.context.guide:AGENTS.md".into(),
            json!({
                "path":"AGENTS.md","revision":"captured","content":"Exact captured guide"
            }),
        ),
        (
            "sailry.context.declarations".into(),
            json!({
                "instruction":"Exact stable instruction","tools":{}
            }),
        ),
    ]);
    let mut owners = AdkEvent::new("captured-owners");
    owners.author = "tool".into();
    owners.set_content(Content {
        role: "user".into(),
        parts: [
            ("earlier-plan", "update_plan", json!({"progress":{
                "title":"Earlier plan", "steps":[{"description":"Superseded step","state":"pending"}]
            }})),
            ("current-plan", "update_plan", json!({"progress":{
                "title":"Captured plan", "steps":[{"description":"Exact pending step","state":"pending"}]
            }})),
            ("loaded-skill", "load_skill", json!({
                "skill":"fixture:review","path":"SKILL.md","content":"Exact loaded skill"
            })),
            ("loaded-resource", "read_skill_resource", json!({
                "skill":"fixture:review","path":"reference.md","content":"Exact loaded resource"
            })),
        ].into_iter().map(|(id,name,value)| AdkPart::FunctionResponse {
            id:Some(id.into()),
            function_response:FunctionResponseData::new(name,value),
            annotations:None,
        }).collect(),
    });
    let mut selected = AdkEvent::new("selected-skill");
    selected.author = "user".into();
    selected
        .provider_metadata
        .insert(CAPABILITY_PART.into(), "1".into());
    selected.set_content(
        Content::new("user")
            .with_text("Original user task")
            .with_text("Exact selected skill workflow"),
    );
    let anchor = event("Completed source evidence");
    fixture
        .append([captured, owners, selected, anchor.clone()])
        .await;
    let compacted = summary(&anchor);
    let stored = serde_json::to_vec(&compacted).unwrap();
    let turn = fixture.append([compacted.clone()]).await;
    let first = load(&fixture, turn).await;
    let content = first[0].content().unwrap();
    assert_eq!(content.role, "user");
    let text = serde_json::to_string(content).unwrap();
    for marker in [
        "Exact captured policy",
        "Exact captured environment",
        "Exact captured guide",
        "Exact pending step",
        "Exact loaded skill",
        "Exact loaded resource",
        "Exact selected skill workflow",
    ] {
        assert_eq!(text.matches(marker).count(), 1, "{marker}");
        assert!(!String::from_utf8_lossy(&stored).contains(marker));
    }
    assert!(!text.contains("Superseded step"));
    assert_eq!(
        compacted
            .actions
            .compaction
            .as_ref()
            .unwrap()
            .compacted_content
            .parts
            .len(),
        1
    );
    assert!(compacted.actions.state_delta.is_empty());
    assert_eq!(
        first[0].actions.state_delta["sailry.context.declarations"]["instruction"],
        "Exact stable instruction"
    );
    assert_eq!(
        first[0].actions.state_delta["sailry.context.masked_outputs"],
        json!([["retained-call", "captured-revision"]])
    );
    history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    let reloaded = load(&fixture, turn).await;
    assert_eq!(
        serde_json::to_vec(&first[0].content()).unwrap(),
        serde_json::to_vec(&reloaded[0].content()).unwrap()
    );
    let body: Vec<u8> = fixture
        .database
        .connection
        .prepare("SELECT body FROM agent_events WHERE id=?1 AND turn=?2")
        .await
        .unwrap()
        .query_row(params![compacted.id.as_str(), turn.to_string()])
        .await
        .unwrap()
        .get(0)
        .unwrap();
    let saved: AdkEvent = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        saved
            .actions
            .compaction
            .unwrap()
            .compacted_content
            .parts
            .len(),
        1
    );
    assert!(saved.actions.state_delta.is_empty());
}

#[tokio::test]
async fn retains_only_latest_input_capability() {
    for newer in [false, true] {
        let mut fixture = Fixture::new().await;
        let selected = |text: &str| {
            let mut event = AdkEvent::new("selected-capability");
            event.author = "user".into();
            event
                .provider_metadata
                .insert(CAPABILITY_PART.into(), "1".into());
            event.set_content(
                adk_core::Content::new("user")
                    .with_text("User task")
                    .with_text(text),
            );
            event
        };
        let anchor = event("Completed evidence");
        fixture
            .append([
                selected("Earlier selection"),
                selected("Latest selection"),
                anchor.clone(),
            ])
            .await;
        if newer {
            let mut input = AdkEvent::new("unselected-input");
            input.author = "user".into();
            input.set_content(
                adk_core::Content::new("user").with_text("Next task without a selection"),
            );
            fixture.append([input]).await;
        }
        let turn = fixture.append([summary(&anchor)]).await;
        let loaded = load(&fixture, turn).await;
        let text = serde_json::to_string(loaded[0].content().unwrap()).unwrap();
        assert!(!text.contains("Earlier selection"));
        assert_eq!(text.contains("Latest selection"), !newer);
    }
}

#[tokio::test]
async fn retains_capability_across_guidance_and_notices() {
    use adk_core::Content;

    let mut fixture = Fixture::new().await;
    let owner = fixture.queued().await;
    runs::start(&fixture.database.connection, owner)
        .await
        .unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    let mut selected = AdkEvent::new("selected-capability");
    selected.author = "user".into();
    selected
        .provider_metadata
        .insert(CAPABILITY_PART.into(), "1".into());
    selected.set_content(
        Content::new("user")
            .with_text("User task")
            .with_text("Exact capability survives guidance"),
    );
    history::append(
        &mut fixture.database,
        fixture.session,
        owner,
        selected,
        &fixture.events,
    )
    .await
    .unwrap();
    let Output::QueuedTurn(guidance) = command(
        &mut fixture.database,
        &fixture.events,
        Command::GuideTurn {
            recipient: owner,
            expected_revision: 1,
            message: "Continue this task".into(),
        },
    )
    .await
    else {
        panic!("guided turn expected")
    };
    let claimed = runs::guidance::take(&mut fixture.database, owner, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed[0].turn.id, guidance.id);
    let mut guided = AdkEvent::new("guidance-input");
    guided.author = "user".into();
    guided.set_content(Content::new("user").with_text("Continue this task"));
    history::append(
        &mut fixture.database,
        fixture.session,
        guidance.id,
        guided,
        &fixture.events,
    )
    .await
    .unwrap();
    let anchor = event("Completed guided evidence");
    history::append(
        &mut fixture.database,
        fixture.session,
        owner,
        anchor.clone(),
        &fixture.events,
    )
    .await
    .unwrap();
    runs::finish(
        &mut fixture.database,
        owner,
        Status::Completed,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    let mut notice = AdkEvent::new("notice-input");
    notice.author = "user".into();
    notice.set_content(Content::new("user").with_text("Background task completed"));
    let notification = fixture.append([notice]).await;
    let mut run = runs::get(&fixture.database.connection, notification)
        .await
        .unwrap();
    run.notice = true;
    runs::put(&fixture.database.connection, &run).await.unwrap();
    let turn = fixture.append([summary(&anchor)]).await;
    let loaded = load(&fixture, turn).await;
    let text = serde_json::to_string(loaded[0].content().unwrap()).unwrap();
    assert_eq!(
        text.matches("Exact capability survives guidance").count(),
        1
    );
    assert!(!text.contains("Continue this task"));
    assert_eq!(
        loaded[1].content().unwrap().parts[0].text(),
        Some("Background task completed")
    );
}

#[tokio::test]
async fn excludes_capability_after_the_boundary() {
    use adk_core::{Content, FunctionResponseData, Part as AdkPart};

    let mut fixture = Fixture::new().await;
    let selected = |text: &str| {
        let mut selected = AdkEvent::new("selected-capability");
        selected.author = "user".into();
        selected
            .provider_metadata
            .insert(CAPABILITY_PART.into(), "1".into());
        selected.set_content(Content::new("user").with_text("User task").with_text(text));
        selected
    };
    let mut call = event("");
    call.set_content(Content {
        role: "model".into(),
        parts: vec![AdkPart::FunctionCall {
            id: Some("boundary-read".into()),
            name: "read_file".into(),
            args: serde_json::json!({"path":"fixture.txt"}),
            thought_signature: None,
        }],
    });
    let mut anchor = event("");
    anchor.set_content(Content {
        role: "user".into(),
        parts: vec![AdkPart::FunctionResponse {
            id: Some("boundary-read".into()),
            function_response: FunctionResponseData::new(
                "read_file",
                serde_json::json!({"content":"Earlier completed evidence"}),
            ),
            annotations: None,
        }],
    });
    fixture
        .append([
            selected("Capability before the boundary"),
            call,
            anchor.clone(),
            selected("Capability after the boundary"),
        ])
        .await;
    let turn = fixture.append([summary(&anchor)]).await;
    let loaded = load(&fixture, turn).await;
    let before = serde_json::to_string(loaded[0].content().unwrap()).unwrap();
    assert_eq!(before.matches("Capability before the boundary").count(), 1);
    assert!(!before.contains("Capability after the boundary"));
    let after = serde_json::to_string(loaded[1].content().unwrap()).unwrap();
    assert_eq!(after.matches("Capability after the boundary").count(), 1);
}

#[tokio::test]
async fn merges_disjoint_discovered_tools() {
    let mut fixture = Fixture::new().await;
    let discovered = |name: &str| {
        let mut event = AdkEvent::new("discovered-tools");
        event.author = "system".into();
        event
            .actions
            .state_delta
            .insert(LOADED_TOOLS.into(), serde_json::json!([name]));
        event
    };
    let anchor = event("Completed discovery");
    fixture
        .append([discovered("first"), discovered("second"), anchor.clone()])
        .await;
    let turn = fixture.append([summary(&anchor)]).await;
    let loaded = load(&fixture, turn).await;
    assert!(!loaded[0].actions.state_delta.contains_key(LOADED_TOOLS));
    for _ in 0..2 {
        assert_eq!(
            history::state(&fixture.database.connection, fixture.session)
                .await
                .unwrap()[LOADED_TOOLS],
            serde_json::json!(["first", "second"])
        );
        history::rebuild_session(&fixture.database.connection, fixture.session)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn ignores_clock_rewinds() {
    let mut fixture = Fixture::new().await;
    let original = event("Original evidence");
    fixture.append([original.clone()]).await;
    let mut recent = event("Recent must survive");
    recent.timestamp = original.timestamp - chrono::Duration::seconds(60);
    fixture.append([recent.clone()]).await;
    let compacted = summary(&original);
    let turn = fixture.append([event("Current"), compacted.clone()]).await;
    let full = fixture.read(None, 100).await.page;
    let context = load(&fixture, turn).await;
    assert_eq!(context.len(), 3);
    assert_eq!(context[0].id, compacted.id);
    assert!(context[0].actions.compaction.is_none());
    assert_eq!(context[1].id, recent.id);
    assert!(!context.iter().any(|event| event.id == original.id));
    assert_eq!(full.entries.len(), 4);
    assert_eq!(
        full.entries.last().unwrap().parts,
        vec![Part::Compaction("Summary 中文 🙂".into())]
    );
    history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    assert_eq!(fixture.read(None, 100).await.page, full);
    assert_eq!(
        load(&fixture, turn)
            .await
            .iter()
            .map(|event| &event.id)
            .collect::<Vec<_>>(),
        context.iter().map(|event| &event.id).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn rejects_invalid_boundaries() {
    let mut fixture = Fixture::new().await;
    let first = event("First");
    let last = event("Last");
    fixture.append([first.clone(), last.clone()]).await;
    let turn = fixture.queued().await;
    for boundary in [first, event("Not a member")] {
        assert!(
            history::append(
                &mut fixture.database,
                fixture.session,
                turn,
                summary(&boundary),
                &fixture.events
            )
            .await
            .is_err()
        );
    }
    let current = event("Current");
    history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        current.clone(),
        &fixture.events,
    )
    .await
    .unwrap();
    assert!(
        history::append(
            &mut fixture.database,
            fixture.session,
            turn,
            summary(&current),
            &fixture.events
        )
        .await
        .is_err()
    );
    assert_eq!(fixture.read(None, 100).await.page.entries.len(), 3);
    history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        summary(&last),
        &fixture.events,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn restores_branched_context() {
    let mut fixture = Fixture::new().await;
    let original = event("Original");
    let first = fixture.append([original.clone()]).await;
    let compacted = summary(&original);
    let head = fixture.append([event("Recent"), compacted.clone()]).await;
    let Output::Session(branch) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: fixture.session,
            through: head,
            expected_revision: 1,
        },
    )
    .await
    else {
        panic!("fork expected")
    };
    let source = fixture.session;
    fixture.session = branch.id;
    let branch_turn = fixture.queued().await;
    assert_eq!(load(&fixture, branch_turn).await[0].id, compacted.id);
    fixture.session = source;
    let retained = fixture.fork(source, head).await;
    let Output::Rewound(rewound) = command(
        &mut fixture.database,
        &fixture.events,
        Command::RewindConversation {
            session: source,
            through: Some(first),
            expected_head: head,
            expected_revision: 1,
        },
    )
    .await
    else {
        panic!("rewind expected")
    };
    let context = load(&fixture, first).await;
    assert!(rewound.backup.is_none());
    assert_eq!(context.len(), 1);
    assert_eq!(context[0].id, original.id);
    fixture.session = retained.id;
    let backup_turn = fixture.queued().await;
    assert_eq!(load(&fixture, backup_turn).await[0].id, compacted.id);
    assert_eq!(fixture.read(None, 100).await.page.entries.len(), 3);
}

#[tokio::test]
async fn loads_complete_history() {
    let mut fixture = Fixture::new().await;
    let turn = fixture.completed(&["Preserved"; 300]).await;
    assert_eq!(load(&fixture, turn).await.len(), 300);
}

#[tokio::test]
async fn accepts_complete_batches() {
    use adk_core::{Content, FunctionResponseData, Part as AdkPart};
    let mut fixture = Fixture::new().await;
    let turn = fixture.queued().await;
    runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    let mut call = event("");
    call.set_content(Content {
        role: "model".into(),
        parts: ["first", "second"]
            .map(|id| AdkPart::FunctionCall {
                id: Some(id.into()),
                name: "read_file".into(),
                args: serde_json::json!({"path":id}),
                thought_signature: None,
            })
            .into(),
    });
    let results = ["first", "second"].map(|id| {
        let mut result = event("");
        result.set_content(Content {
            role: "user".into(),
            parts: vec![AdkPart::FunctionResponse {
                id: Some(id.into()),
                function_response: FunctionResponseData::new(
                    "read_file",
                    serde_json::json!({"content":id}),
                ),
                annotations: None,
            }],
        });
        result
    });
    for boundary in [&call, &results[0]] {
        history::append(
            &mut fixture.database,
            fixture.session,
            turn,
            boundary.clone(),
            &fixture.events,
        )
        .await
        .unwrap();
        assert!(
            history::append(
                &mut fixture.database,
                fixture.session,
                turn,
                summary(boundary),
                &fixture.events
            )
            .await
            .is_err()
        );
    }
    history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        results[1].clone(),
        &fixture.events,
    )
    .await
    .unwrap();
    let mut telemetry = AdkEvent::new("timing-fixture");
    telemetry.author = "system".into();
    telemetry.provider_metadata.insert(
        "sailry_execution_phases".into(),
        serde_json::to_string(&sailry_protocol::conversation::observation::Phases::default())
            .unwrap(),
    );
    history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        telemetry.clone(),
        &fixture.events,
    )
    .await
    .unwrap();
    assert!(
        context::validate(
            &fixture.database.connection,
            fixture.session,
            turn,
            &summary(&telemetry),
        )
        .await
        .is_err()
    );
    let compacted = summary(&results[1]);
    history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        compacted.clone(),
        &fixture.events,
    )
    .await
    .unwrap();
    let recent = event("Recent evidence");
    history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        recent.clone(),
        &fixture.events,
    )
    .await
    .unwrap();
    let current = load(&fixture, turn).await;
    assert_eq!(
        current.iter().map(|event| &event.id).collect::<Vec<_>>(),
        [&compacted.id, &recent.id]
    );
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    assert!(
        context::validate(
            &fixture.database.connection,
            fixture.session,
            turn,
            &summary(&telemetry),
        )
        .await
        .is_err()
    );
    history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        compacted,
        &fixture.events,
    )
    .await
    .unwrap();
    let full = fixture.read(None, 100).await.page;
    assert_eq!(full.entries.len(), 5);
    history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    assert_eq!(fixture.read(None, 100).await.page, full);
}

#[tokio::test]
async fn rejects_orphans_and_mismatches() {
    use adk_core::{Content, FunctionResponseData, Part as AdkPart};
    let mut call = event("");
    call.set_content(Content {
        role: "model".into(),
        parts: vec![AdkPart::FunctionCall {
            id: Some("call".into()),
            name: "read_file".into(),
            args: serde_json::json!({}),
            thought_signature: None,
        }],
    });
    for (id, name) in [
        ("wrong", "read_file"),
        ("call", "wrong"),
        ("call", "read_file"),
    ] {
        let mut result = event("");
        result.set_content(Content {
            role: "user".into(),
            parts: vec![AdkPart::FunctionResponse {
                id: Some(id.into()),
                function_response: FunctionResponseData::new(name, serde_json::json!({})),
                annotations: None,
            }],
        });
        assert!(!context::complete_exchange(&[result.clone()]));
        assert_eq!(
            context::complete_exchange(&[call.clone(), result]),
            id == "call" && name == "read_file"
        );
    }
}

#[tokio::test]
async fn uses_real_turn_boundaries() {
    let mut fixture = Fixture::new().await;
    let original = event("First canonical content");
    let mut retry = AdkEvent::new("different-synthetic-invocation");
    retry.author = "system".into();
    retry.provider_metadata.insert(
        "sailry_retry".into(),
        serde_json::json!({"attempt":1,"limit":5}).to_string(),
    );
    let final_event = event("Final canonical content");
    let mut telemetry = AdkEvent::new("timing-fixture");
    telemetry.author = "system".into();
    telemetry.provider_metadata.insert(
        "sailry_execution_phases".into(),
        serde_json::to_string(&sailry_protocol::conversation::observation::Phases::default())
            .unwrap(),
    );
    let mut projection = AdkEvent::new("projection-fixture");
    projection.author = "system".into();
    projection.provider_metadata.insert(
        "sailry_context_projection".into(),
        serde_json::json!({"tokens":128}).to_string(),
    );
    projection.actions.state_delta.insert(
        "sailry.context.masked_outputs".into(),
        serde_json::json!([["call", "revision"]]),
    );
    fixture
        .append([
            original.clone(),
            retry,
            final_event.clone(),
            telemetry,
            projection,
        ])
        .await;
    let current = fixture.queued().await;
    let history = context::history(&fixture.database.connection, fixture.session, current)
        .await
        .unwrap();
    assert_eq!(history.completed, vec![3]);
    assert_eq!(history.events.len(), 3);
    assert_eq!(history.current, 3);
    assert_eq!(history.recent, None);
    assert_eq!(history.events[0].id, original.id);
    assert_eq!(history.events[2].id, final_event.id);
    assert!(
        context::validate(
            &fixture.database.connection,
            fixture.session,
            current,
            &summary(&original)
        )
        .await
        .is_err()
    );
    assert!(
        context::validate(
            &fixture.database.connection,
            fixture.session,
            current,
            &summary(&final_event)
        )
        .await
        .is_ok()
    );
}

#[tokio::test]
async fn retains_recent_plain_turns() {
    let mut fixture = Fixture::new().await;
    fixture.append([event("First turn")]).await;
    let second = event("Second turn");
    fixture
        .append([second.clone(), event("Second response")])
        .await;
    fixture.append([event("Third turn")]).await;
    let current = fixture.queued().await;
    let history = context::history(&fixture.database.connection, fixture.session, current)
        .await
        .unwrap();
    assert_eq!(history.recent, Some(1));
    assert_eq!(history.events[history.recent.unwrap()].id, second.id);
    assert_eq!(history.completed, vec![1, 3, 4]);
}

#[tokio::test]
async fn ignores_contentless_attempts_for_recency() {
    let mut fixture = Fixture::new().await;
    fixture.append([event("Original completed evidence")]).await;
    let mut notice = AdkEvent::new("failed-preparation");
    notice.author = "system".into();
    notice.provider_metadata.insert(
        "sailry_compaction_failure".into(),
        serde_json::json!({"source":"automatic", "stage":"request", "reason":"service_rejected", "continued":false}).to_string(),
    );
    fixture.append([notice]).await;
    let current = fixture.queued().await;
    let history = context::history(&fixture.database.connection, fixture.session, current)
        .await
        .unwrap();
    assert_eq!(history.completed, [1]);
    assert_eq!(history.recent, None);
}
