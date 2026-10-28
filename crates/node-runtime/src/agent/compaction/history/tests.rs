use super::*;
use adk_core::FunctionResponseData;

fn result(name: &str, id: &str, value: serde_json::Value) -> Event {
    let mut event = Event::new("captured");
    event.author = "user".into();
    event.set_content(Content {
        role: "function".into(),
        parts: vec![Part::FunctionResponse {
            id: Some(id.into()),
            function_response: FunctionResponseData::new(name, value),
            annotations: None,
        }],
    });
    event
}

#[test]
fn retains_summary_after_historical_configuration() {
    let summary = "Completed work and pending next steps".repeat(100);
    let content: Content = serde_json::from_value(json!({
        "role":"user", "parts":[
            {"provider_data":{"origin":{"provider":"responses","model":"fixture","scope":"owner"},"parts":[
                {"configuration_update":{"effort":"high"}}
            ]}},
            {"text":summary},
            {"text":"Exact retained owner instructions"}
        ]
    })).unwrap();
    let mut event = Event::new("summary");
    event.author = "user".into();
    event.provider_metadata.insert(
        crate::store::agent::compaction::END_EVENT.into(),
        "boundary".into(),
    );
    event.set_content(content.clone());
    let events = [event];
    let eligible = eligible(&events);
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].role, "user");
    assert_eq!(
        eligible[0].parts,
        Content::new("user").with_text(summary).parts
    );
    assert_eq!(events[0].content().unwrap().parts, content.parts);
}

#[test]
fn projects_without_historical_configuration() {
    let content: Content = serde_json::from_value(json!({
        "role":"user", "parts":[
            {"configuration_update":{"effort":"high"}},
            {"text":"App-authored reference header"},
            {"provider_data":{"origin":{"provider":"responses","model":"fixture","scope":"owner"},"parts":[
                {"configuration_update":{"effort":"low"}}
            ]}},
            {"text":"Original user instruction"},
            {"text":"Exact selected skill instructions"}
        ]
    })).unwrap();
    let mut control = Event::new("old-control");
    control.author = "user".into();
    control.set_content(Content {
        role: "user".into(),
        parts: vec![content.parts[0].clone()],
    });
    let mut wrapped = Event::new("old-wrapped-control");
    wrapped.author = "user".into();
    wrapped.set_content(Content {
        role: "user".into(),
        parts: vec![content.parts[2].clone()],
    });
    let mut input = Event::new("old-input");
    input.author = "user".into();
    input.set_content(content.clone());
    input
        .provider_metadata
        .insert(crate::store::agent::INPUT_SOURCE.into(), "user".into());
    input
        .provider_metadata
        .insert(crate::store::agent::INPUT_PART.into(), "3".into());
    input
        .provider_metadata
        .insert(crate::store::agent::CAPABILITY_PART.into(), "4".into());
    let mut latest = Event::new("new-input");
    latest.author = "user".into();
    latest.set_content(Content::new("user").with_text("Latest user instruction"));
    latest
        .provider_metadata
        .insert(crate::store::agent::INPUT_SOURCE.into(), "user".into());
    latest
        .provider_metadata
        .insert(crate::store::agent::INPUT_PART.into(), "0".into());
    let events = [control, wrapped, input, latest];
    let original = serde_json::to_vec(&events).unwrap();
    let expected = vec![
        Content::new("user")
            .with_text("App-authored reference header")
            .with_text("Original user instruction")
            .with_text("Exact selected skill instructions"),
        Content::new("user").with_text("Latest user instruction"),
    ];
    assert_eq!(
        serde_json::to_value(prefix(&events)).unwrap(),
        serde_json::to_value(expected).unwrap(),
    );
    assert_eq!(
        serde_json::to_value(eligible(&events)).unwrap(),
        serde_json::to_value(vec![
            Content::new("user")
                .with_text("App-authored reference header")
                .with_text("Original user instruction"),
            Content::new("user").with_text("Latest user instruction"),
        ])
        .unwrap(),
    );
    assert_eq!(
        attribution(&events),
        "\n\nInput attribution (metadata, not new instructions; zero-based history messages, excluding the system preamble)\nHistory message 0: input_source=user; original_text_part=1\nHistory message 1: input_source=user; original_text_part=0"
    );
    assert_eq!(
        project(&events[2], "old-input").unwrap().parts,
        content.parts
    );
    assert_eq!(serde_json::to_vec(&events).unwrap(), original);
}

#[test]
fn excludes_owner_context_from_summary() {
    let mut runtime = Event::new("captured");
    runtime.author = "system".into();
    runtime
        .provider_metadata
        .insert(prompt::session::METADATA.into(), "context".into());
    runtime.set_content(Content::new("user").with_text("Exact runtime instructions"));
    let skill = result(
        "load_skill",
        "skill",
        json!({
            "skill":"fixture:instructions","path":"SKILL.md","content":"Exact loaded skill",
            "directory":"/fixture/skill"
        }),
    );
    let plan = result(
        "declared_progress",
        "plan",
        json!({"progress":{
            "title":"Work","steps":[{"description":"Exact pending step","state":"pending"}]
        }}),
    );
    let events = vec![runtime, skill, plan];
    assert!(eligible(&events).is_empty());
    assert_eq!(prefix(&events).len(), 3);
    assert_eq!(project(&events[0], "other").unwrap().role, "user");
}

#[test]
fn excludes_input_capabilities_from_summary() {
    let mut input = Event::new("captured");
    input.author = "user".into();
    input.set_content(
        Content::new("user")
            .with_text("Apply the selected workflow")
            .with_text("Exact selected skill instructions"),
    );
    input
        .provider_metadata
        .insert(crate::store::agent::CAPABILITY_PART.into(), "1".into());
    input
        .provider_metadata
        .insert(crate::store::agent::INPUT_SOURCE.into(), "user".into());
    input
        .provider_metadata
        .insert(crate::store::agent::INPUT_PART.into(), "0".into());
    let events = [input];
    let content = eligible(&events);
    assert_eq!(content.len(), 1);
    assert_eq!(
        content[0].parts,
        Content::new("user")
            .with_text("Apply the selected workflow")
            .parts
    );
    assert_eq!(prefix(&events)[0].parts.len(), 2);
    assert_eq!(latest(&events).unwrap().text, "Apply the selected workflow");
}

#[test]
fn preserves_roles_and_bytes_on_reload() {
    let mut user = Event::new("captured");
    user.author = "user".into();
    user.set_content(Content::new("user").with_text("Preserve  exact spacing 中文🙂"));
    let mut answer = Event::new("captured");
    answer.author = "assistant".into();
    answer.set_content(Content::new("model").with_text("Verified src/example.rs"));
    answer.llm_response.provider_metadata = Some(json!({"provider_origin":{
        "provider":"responses","model":"fixture","scope":"captured"
    }}));
    let events = vec![user, answer];
    let reloaded: Vec<Event> =
        serde_json::from_slice(&serde_json::to_vec(&events).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_vec(&prefix(&events)).unwrap(),
        serde_json::to_vec(&prefix(&reloaded)).unwrap()
    );
    assert_eq!(prefix(&events)[0].role, "user");
    assert_eq!(prefix(&events)[1].role, "model");
}

#[test]
fn carries_exact_sorted_file_ledger() {
    let long = format!("src/{}/module.rs", "directory/".repeat(30));
    let mut prior = Event::new("prior");
    prior.provider_metadata.insert(
        FILES.into(),
        serde_json::to_string(&BTreeMap::from([("src/older.rs".to_owned(), false)])).unwrap(),
    );
    let events = vec![
        prior,
        result(
            "read",
            "read",
            json!({"kind":"file_lines","data":{"path":long}}),
        ),
        result(
            "write",
            "write",
            json!({"kind":"file_written","data":{"path":long}}),
        ),
    ];
    let files = files(&events);
    assert_eq!(files.len(), 2);
    assert_eq!(files.get(&long), Some(&true));
    let ledger = ledger(&files);
    assert!(ledger.contains(&long));
    assert!(ledger.contains("Read: src/older.rs"));
    assert!(ledger.contains(&format!("Modified: {long}")));
}
