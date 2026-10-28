use super::super::compaction as context;
use super::{fixture::Fixture, *};
use crate::agent::canonical::{self as records, BlockKind, Kind, Record, Role};
use fixture::{message, record, request, response};
use serde_json::json;

fn event(text: &str) -> Record {
    message(
        "assistant",
        Role::Assistant,
        vec![BlockKind::Text { text: text.into() }],
    )
}

fn summary(end: &Record) -> Record {
    let content = message(
        "user",
        Role::User,
        vec![BlockKind::Text {
            text: "Summary 中文 🙂".into(),
        }],
    );
    let mut compacted = record(
        "system",
        Kind::Compaction(records::Compaction {
            boundary: end.id.clone(),
            summary: content.message().unwrap().clone(),
            start: end.created,
            end: end.created,
            files: Vec::new(),
        }),
    );
    compacted.metadata.runtime = Some(records::Runtime {
        user_visible: false,
        ..Default::default()
    });
    compacted
}

fn capability(text: &str) -> Record {
    let mut selected = message(
        "user",
        Role::User,
        vec![
            BlockKind::Text {
                text: "User task".into(),
            },
            BlockKind::Text { text: text.into() },
        ],
    );
    selected.metadata.input = Some(records::Input {
        capability_part: Some(1),
        ..Default::default()
    });
    selected
}

async fn load(fixture: &Fixture, turn: TurnId) -> sessions::Session {
    sessions::get(
        &fixture.database.connection,
        turn,
        sessions::Query {
            session: fixture.session,
            recent: None,
            after: None,
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn limits_plugin_initialization_to_captured_reads() {
    let mut fixture = Fixture::new().await;
    fixture::package(&fixture.database, fixture._root.path(), "goals").await;
    let package = crate::store::plugins::get(&fixture.database.connection, "goals")
        .await
        .unwrap()
        .unwrap();
    if !package.summary.enabled {
        command(
            &mut fixture.database,
            &fixture.events,
            Command::SetPluginEnabled {
                name: "goals".into(),
                expected_revision: package.summary.revision,
                enabled: true,
            },
        )
        .await;
    }
    fixture.completed(&["Completed evidence"]).await;
    let Output::QueuedTurn(turn) = command(
        &mut fixture.database,
        &fixture.events,
        Command::CompactContext {
            session: fixture.session,
            expected_revision: 1,
            focus: None,
        },
    )
    .await
    else {
        panic!("compaction turn expected")
    };
    let invocation = runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(invocation.turn.id, turn.id);
    let context = plugin::Context {
        package: turn
            .plugins
            .iter()
            .find(|package| package.name == "goals")
            .unwrap()
            .clone(),
        session: Some(fixture.session),
        worktree: Some(invocation.worktree),
        turn: Some(turn.id),
        invocation: None,
        surface: Default::default(),
    };
    let read = Request::new(
        fixture.database.node,
        Command::ReadPluginConversationValue { key: "goal".into() },
    )
    .with_plugin(context.clone());
    let db = &fixture.database.connection;
    assert_eq!(
        plugins::package(db, &read)
            .await
            .unwrap()
            .summary
            .reference(),
        context.package
    );
    assert!(calls::active(db, turn.id).await.is_err());
    for command in [
        Command::WritePluginConversationValue {
            key: "goal".into(),
            value: serde_json::json!({"state":"completed"}),
            expected_revision: 0,
        },
        Command::CallPlugin {
            handler: "read".into(),
            input: serde_json::json!({}),
        },
    ] {
        let request = Request::new(fixture.database.node, command).with_plugin(context.clone());
        assert_eq!(
            plugins::package(db, &request).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
    for changed in [
        plugin::Context {
            session: Some(SessionId::new()),
            ..context.clone()
        },
        plugin::Context {
            package: plugin::Reference {
                digest: "outside-capture".into(),
                ..context.package.clone()
            },
            ..context.clone()
        },
    ] {
        let mut request = read.clone();
        request.plugin = Some(changed);
        assert_eq!(
            plugins::package(db, &request).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
    }
    runs::finish(
        &mut fixture.database,
        turn.id,
        Status::Completed,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    assert_eq!(
        plugins::package(&fixture.database.connection, &read)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[tokio::test]
async fn retains_summary_only_update_time() {
    let mut fixture = Fixture::new().await;
    let original = event("Original evidence");
    fixture.append([original.clone()]).await;
    let compacted = summary(&original);
    let turn = fixture.append([compacted.clone()]).await;
    let session = load(&fixture, turn).await;
    assert_eq!(session.records.last().unwrap().created, compacted.created);
    let visible = session.conversation.agent_visible_messages();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].created, compacted.created.timestamp());
}

#[tokio::test]
async fn retains_canonical_owner_sources() {
    let mut fixture = Fixture::new().await;
    let captured = record(
        "system",
        Kind::Configuration(records::Configuration {
            context: records::Context {
                captured: true,
                policy: Some("Exact captured policy".into()),
                environment: Some("Exact captured environment".into()),
                guides: [(
                    "sailry.context.guide:AGENTS.md".into(),
                    records::Guide {
                        path: "AGENTS.md".into(),
                        revision: "captured".into(),
                        content: "Exact captured guide".into(),
                    },
                )]
                .into(),
                ..Default::default()
            },
        }),
    );
    let owners = message("tool", Role::User, [
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
    ].into_iter().map(|(id, name, value)| {
        BlockKind::ToolResponse(response(Some(id), name, value))
    }).collect());
    let selected = capability("Exact selected skill workflow");
    let anchor = event("Completed source evidence");
    fixture
        .append([captured, owners.clone(), selected.clone(), anchor.clone()])
        .await;
    let compacted = summary(&anchor);
    let turn = fixture.append([compacted.clone()]).await;
    let first = load(&fixture, turn).await;
    assert_eq!(
        first.context.policy.as_deref(),
        Some("Exact captured policy")
    );
    assert_eq!(
        first.context.environment.as_deref(),
        Some("Exact captured environment")
    );
    assert_eq!(
        first.context.guides["sailry.context.guide:AGENTS.md"].content,
        "Exact captured guide"
    );
    for original in [&owners, &selected] {
        let saved = first
            .records
            .iter()
            .find(|record| record.id == original.id)
            .unwrap();
        assert_eq!(
            serde_json::to_value(&saved.kind).unwrap(),
            serde_json::to_value(&original.kind).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&saved.metadata.input).unwrap(),
            serde_json::to_value(&original.metadata.input).unwrap()
        );
    }
    let active = first.conversation.agent_visible_messages();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].as_concat_text(), "Summary 中文 🙂");
    let serialized = serde_json::to_string(&compacted).unwrap();
    for marker in [
        "Exact captured policy",
        "Exact captured environment",
        "Exact captured guide",
        "Exact pending step",
        "Exact loaded skill",
        "Exact loaded resource",
        "Exact selected skill workflow",
    ] {
        assert!(!serialized.contains(marker), "{marker}");
    }
    history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    let reloaded = load(&fixture, turn).await;
    assert_eq!(
        serde_json::to_value(&first.context).unwrap(),
        serde_json::to_value(&reloaded.context).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&first.records).unwrap(),
        serde_json::to_value(&reloaded.records).unwrap()
    );
    assert_eq!(
        reloaded.conversation.agent_visible_messages()[0].as_concat_text(),
        "Summary 中文 🙂"
    );
    let saved = reloaded
        .records
        .iter()
        .find(|record| record.id == compacted.id)
        .unwrap();
    let Kind::Compaction(summary) = &saved.kind else {
        panic!("canonical compaction expected")
    };
    assert_eq!(summary.summary.content.len(), 1);
    assert!(saved.metadata.context.is_none());
    assert!(saved.metadata.state.is_none());
}

#[tokio::test]
async fn retains_input_capabilities() {
    for newer in [false, true] {
        let mut fixture = Fixture::new().await;
        let earlier = capability("Earlier selection");
        let latest = capability("Latest selection");
        let anchor = event("Completed evidence");
        fixture
            .append([earlier.clone(), latest.clone(), anchor.clone()])
            .await;
        if newer {
            fixture
                .append([message(
                    "user",
                    Role::User,
                    vec![BlockKind::Text {
                        text: "Next task without a selection".into(),
                    }],
                )])
                .await;
        }
        let turn = fixture.append([summary(&anchor)]).await;
        let loaded = load(&fixture, turn).await;
        for original in [&earlier, &latest] {
            let saved = loaded
                .records
                .iter()
                .find(|record| record.id == original.id)
                .unwrap();
            assert_eq!(
                serde_json::to_value(saved.message()).unwrap(),
                serde_json::to_value(original.message()).unwrap()
            );
            assert_eq!(
                saved.metadata.input.as_ref().unwrap().capability_part,
                Some(1)
            );
        }
        assert_eq!(loaded.conversation.agent_visible_messages().len(), 1);
        assert_eq!(
            loaded.conversation.agent_visible_messages()[0].as_concat_text(),
            "Summary 中文 🙂"
        );
    }
}

#[tokio::test]
async fn retains_capability_across_guidance_and_notices() {
    let mut fixture = Fixture::new().await;
    let owner = fixture.queued().await;
    runs::start(&fixture.database.connection, owner)
        .await
        .unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    let mut selected = capability("Exact capability survives guidance");
    selected.turn = owner;
    history::append(
        &mut fixture.database,
        fixture.session,
        owner,
        selected.clone(),
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
    let mut guided = message(
        "user",
        Role::User,
        vec![BlockKind::Text {
            text: "Continue this task".into(),
        }],
    );
    guided.turn = guidance.id;
    history::append(
        &mut fixture.database,
        fixture.session,
        guidance.id,
        guided.clone(),
        &fixture.events,
    )
    .await
    .unwrap();
    let mut anchor = event("Completed guided evidence");
    anchor.turn = owner;
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
    let notice = message(
        "user",
        Role::User,
        vec![BlockKind::Text {
            text: "Background task completed".into(),
        }],
    );
    let notification = fixture.append([notice.clone()]).await;
    let mut run = runs::get(&fixture.database.connection, notification)
        .await
        .unwrap();
    run.notice = true;
    runs::put(&fixture.database.connection, &run).await.unwrap();
    let turn = fixture.append([summary(&anchor)]).await;
    let loaded = load(&fixture, turn).await;
    let saved = loaded
        .records
        .iter()
        .find(|record| record.id == selected.id)
        .unwrap();
    assert_eq!(
        serde_json::to_value(saved.message()).unwrap(),
        serde_json::to_value(selected.message()).unwrap()
    );
    assert_eq!(
        saved.metadata.input.as_ref().unwrap().capability_part,
        Some(1)
    );
    assert_eq!(
        loaded
            .records
            .iter()
            .find(|record| record.id == guided.id)
            .unwrap()
            .turn,
        guidance.id
    );
    assert_eq!(
        loaded
            .records
            .iter()
            .find(|record| record.id == notice.id)
            .unwrap()
            .message()
            .unwrap()
            .content[0]
            .index,
        0
    );
    assert_eq!(loaded.conversation.agent_visible_messages().len(), 1);
    assert_eq!(
        loaded.capability.as_deref(),
        Some("Exact capability survives guidance")
    );
    let prepared = crate::agent::engine::prompt::prepared(&loaded).await;
    assert_eq!(
        prepared
            .system_prompt
            .matches("Exact capability survives guidance")
            .count(),
        1
    );
    assert!(
        !loaded
            .conversation
            .messages()
            .iter()
            .flat_map(|message| &message.content)
            .filter_map(|content| content.as_text())
            .any(|text| text.contains("Exact capability survives guidance"))
    );
    fixture.database.connection.execute(
        "UPDATE agent_runs SET body=CAST(json_remove(body,'$.notice') AS BLOB) WHERE turn IN (?1,?2)",
        params![owner.to_string(), guidance.id.to_string()],
    ).await.unwrap();
    let earlier = load(&fixture, turn).await;
    assert_eq!(
        serde_json::to_value(&earlier.records).unwrap(),
        serde_json::to_value(&loaded.records).unwrap()
    );
    assert_eq!(
        crate::agent::engine::prompt::prepared(&earlier)
            .await
            .system_prompt,
        prepared.system_prompt
    );
    let next = fixture
        .append([message(
            "user",
            Role::User,
            vec![BlockKind::Text {
                text: "A different task".into(),
            }],
        )])
        .await;
    let changed = load(&fixture, next).await;
    assert!(changed.capability.is_none());
    assert!(
        !crate::agent::engine::prompt::prepared(&changed)
            .await
            .system_prompt
            .contains("Exact capability survives guidance")
    );
}

#[tokio::test]
async fn retains_capabilities_around_the_boundary() {
    let mut fixture = Fixture::new().await;
    let before = capability("Capability before the boundary");
    let after = capability("Capability after the boundary");
    let call = message(
        "assistant",
        Role::Assistant,
        vec![BlockKind::ToolRequest(request(
            Some("boundary-read"),
            "read_file",
            json!({"path":"fixture.txt"}),
        ))],
    );
    let anchor = message(
        "tool",
        Role::User,
        vec![BlockKind::ToolResponse(response(
            Some("boundary-read"),
            "read_file",
            json!({"content":"Earlier completed evidence"}),
        ))],
    );
    fixture
        .append([before.clone(), call, anchor.clone(), after.clone()])
        .await;
    let turn = fixture.append([summary(&anchor)]).await;
    let loaded = load(&fixture, turn).await;
    for original in [&before, &after] {
        let saved = loaded
            .records
            .iter()
            .find(|record| record.id == original.id)
            .unwrap();
        assert_eq!(
            serde_json::to_value(&saved.kind).unwrap(),
            serde_json::to_value(&original.kind).unwrap()
        );
        assert_eq!(
            saved.metadata.input.as_ref().unwrap().capability_part,
            Some(1)
        );
    }
    let (boundary, saved) = context::read(&fixture.database.connection, fixture.session)
        .await
        .unwrap()
        .unwrap();
    let Kind::Compaction(summary) = saved.kind else {
        panic!("compaction expected")
    };
    assert_eq!(summary.boundary, anchor.id);
    let anchored: String = fixture.database.connection.prepare("SELECT e.id FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND h.sequence=?2")
        .await.unwrap().query_row(params![fixture.session.to_string(), boundary]).await.unwrap().get(0).unwrap();
    assert_eq!(anchored, anchor.id);
    assert_eq!(loaded.conversation.agent_visible_messages().len(), 1);
}

#[tokio::test]
async fn merges_disjoint_discovered_tools() {
    let mut fixture = Fixture::new().await;
    let discovered = |name: &str| {
        record(
            "system",
            Kind::Configuration(records::Configuration {
                context: records::Context {
                    loaded_tools: Some([name.into()].into()),
                    ..Default::default()
                },
            }),
        )
    };
    let anchor = event("Completed discovery");
    fixture
        .append([discovered("first"), discovered("second"), anchor.clone()])
        .await;
    let turn = fixture.append([summary(&anchor)]).await;
    for _ in 0..2 {
        let loaded = load(&fixture, turn).await;
        assert_eq!(
            loaded.context.loaded_tools,
            Some(["first".into(), "second".into()].into())
        );
        let saved = loaded.records.last().unwrap();
        assert!(saved.context().is_none());
        assert_eq!(
            history::state(&fixture.database.connection, fixture.session)
                .await
                .unwrap()[LOADED_TOOLS],
            json!(["first", "second"])
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
    recent.created = original.created - chrono::Duration::seconds(60);
    fixture.append([recent.clone()]).await;
    let compacted = summary(&original);
    let current = event("Current");
    let turn = fixture.append([current.clone(), compacted.clone()]).await;
    let full = fixture.read(None, 100).await.page;
    let loaded = load(&fixture, turn).await;
    assert_eq!(
        loaded
            .records
            .iter()
            .map(|record| &record.id)
            .collect::<Vec<_>>(),
        [&original.id, &recent.id, &current.id, &compacted.id]
    );
    let active = loaded.conversation.agent_visible_messages();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id.as_deref(), Some(compacted.id.as_str()));
    assert!(
        loaded.conversation.messages()[..3]
            .iter()
            .all(|message| !message.is_agent_visible())
    );
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
        serde_json::to_value(&load(&fixture, turn).await.records).unwrap(),
        serde_json::to_value(&loaded.records).unwrap()
    );
}

#[tokio::test]
async fn rejects_invalid_boundaries() {
    let mut fixture = Fixture::new().await;
    let original = event("Canonical member");
    fixture.append([original]).await;
    let source = fixture.session;
    let session = crate::store::commands::session(&fixture.database.connection, source)
        .await
        .unwrap();
    let Output::Session(other) = command(
        &mut fixture.database,
        &fixture.events,
        Command::CreateSession {
            project: session.project,
            worktree: Some(session.worktree),
            config: Some(session.config),
        },
    )
    .await
    else {
        panic!("session expected")
    };
    fixture.session = other.id;
    let foreign = event("Foreign member");
    fixture.append([foreign.clone()]).await;
    fixture.session = source;
    let turn = fixture.queued().await;
    for boundary in [event("Not a member"), foreign] {
        let mut compacted = summary(&boundary);
        compacted.turn = turn;
        let fault = history::append(
            &mut fixture.database,
            fixture.session,
            turn,
            compacted,
            &fixture.events,
        )
        .await
        .unwrap_err();
        assert_eq!(fault.code, ErrorCode::InvalidRequest);
    }
    assert_eq!(fixture.read(None, 100).await.page.entries.len(), 1);
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
    let branch = load(&fixture, branch_turn).await;
    assert_eq!(
        branch.conversation.agent_visible_messages()[0]
            .id
            .as_deref(),
        Some(compacted.id.as_str())
    );
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
    let loaded = load(&fixture, first).await;
    assert!(rewound.backup.is_none());
    assert_eq!(loaded.records.len(), 1);
    assert_eq!(loaded.records[0].id, original.id);
    assert_eq!(
        loaded.conversation.agent_visible_messages()[0]
            .id
            .as_deref(),
        Some(original.id.as_str())
    );
    fixture.session = retained.id;
    let backup_turn = fixture.queued().await;
    assert_eq!(
        load(&fixture, backup_turn)
            .await
            .conversation
            .agent_visible_messages()[0]
            .id
            .as_deref(),
        Some(compacted.id.as_str())
    );
    assert_eq!(fixture.read(None, 100).await.page.entries.len(), 3);
}

#[tokio::test]
async fn loads_complete_history() {
    let mut fixture = Fixture::new().await;
    let turn = fixture.completed(&["Preserved"; 300]).await;
    let loaded = load(&fixture, turn).await;
    assert_eq!(loaded.records.len(), 300);
    assert_eq!(loaded.conversation.agent_visible_messages().len(), 300);
}

#[tokio::test]
async fn retains_tool_batches() {
    let mut fixture = Fixture::new().await;
    let turn = fixture.queued().await;
    runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    let mut call = message(
        "assistant",
        Role::Assistant,
        ["first", "second"]
            .map(|id| BlockKind::ToolRequest(request(Some(id), "read_file", json!({"path":id}))))
            .to_vec(),
    );
    call.turn = turn;
    let results = ["first", "second"].map(|id| {
        let mut result = message(
            "tool",
            Role::User,
            vec![BlockKind::ToolResponse(response(
                Some(id),
                "read_file",
                json!({"content":id}),
            ))],
        );
        result.turn = turn;
        result
    });
    for record in [call, results[0].clone(), results[1].clone()] {
        history::append(
            &mut fixture.database,
            fixture.session,
            turn,
            record,
            &fixture.events,
        )
        .await
        .unwrap();
    }
    let mut compacted = summary(&results[1]);
    compacted.turn = turn;
    history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        compacted.clone(),
        &fixture.events,
    )
    .await
    .unwrap();
    let mut recent = event("Recent evidence");
    recent.turn = turn;
    history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        recent.clone(),
        &fixture.events,
    )
    .await
    .unwrap();
    let loaded = load(&fixture, turn).await;
    let active = loaded.conversation.agent_visible_messages();
    assert_eq!(
        active
            .iter()
            .map(|message| message.id.as_deref().unwrap())
            .collect::<Vec<_>>(),
        [compacted.id.as_str(), recent.id.as_str()]
    );
    assert_eq!(
        loaded
            .records
            .iter()
            .filter_map(Record::message)
            .flat_map(|message| &message.content)
            .filter(|block| matches!(block.kind, BlockKind::ToolResponse(_)))
            .count(),
        2
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
