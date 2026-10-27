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
