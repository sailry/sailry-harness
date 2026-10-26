use super::*;
use adk_core::{AdkError, Tool};
use async_trait::async_trait;
use sailry_protocol::{Permission, WorkMode};

struct Fixture {
    description: String,
}

#[async_trait]
impl Tool for Fixture {
    fn name(&self) -> &str {
        "fixture"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(json!({"type":"object"}))
    }

    async fn execute(&self, _: Arc<dyn ToolContext>, _: Value) -> adk_core::Result<Value> {
        Err(AdkError::tool("fixture tools do not execute"))
    }
}

fn tools(description: &str) -> Vec<Registration> {
    vec![Registration::managed(Arc::new(Fixture {
        description: description.into(),
    }))]
}

fn current(mode: WorkMode) -> [(&'static str, String); 3] {
    [
        (
            POLICY,
            super::super::policy(mode, Permission::Ask, false).into(),
        ),
        (ENVIRONMENT, "Environment fixture".into()),
        (PERSONAL, "Personal fixture".into()),
    ]
}

#[test]
fn restores_the_first_request_prefix() {
    let mut snapshot = Snapshot::default();
    let instruction = snapshot.instruction(super::super::baseline().into());
    let declarations = snapshot.declarations(&tools("First description"));
    let captured = serde_json::to_value(&snapshot).unwrap();
    let mut resumed: Snapshot = serde_json::from_value(captured.clone()).unwrap();
    assert_eq!(
        resumed.instruction("Changed instructions".into()),
        instruction
    );
    let restored = resumed.declarations(&tools("Changed description"));
    assert_eq!(
        serde_json::to_value(&restored).unwrap(),
        serde_json::to_value(&declarations).unwrap()
    );
    assert_eq!(serde_json::to_value(resumed).unwrap(), captured);

    let mut request = LlmRequest {
        model: "fixture".into(),
        contents: vec![],
        config: None,
        tools: HashMap::from([("changed".into(), json!({"name":"changed"}))]),
        previous_response_id: None,
    };
    project(&mut request, &declarations);
    let expected = json!({
        "name":"fixture","description":"First description","parameters":{"type":"object"}
    });
    assert_eq!(request.tools.len(), 1);
    assert_eq!(request.tools["fixture"], expected);
    project(&mut request, &restored);
    assert_eq!(request.tools.len(), 1);
    assert_eq!(request.tools["fixture"], expected);
    assert!(!instruction.contains("This turn uses Ask permission"));
    assert!(!instruction.contains("This is a planning turn"));
}

#[test]
fn appends_only_changed_runtime_context() {
    let turn = TurnId::new();
    let mut snapshot = Snapshot::default();
    snapshot.instruction(super::super::baseline().into());
    snapshot.declarations(&tools("First description"));
    let first = capture(
        turn,
        &HashMap::new(),
        &snapshot,
        current(WorkMode::Code),
        &[],
    )
    .unwrap()
    .unwrap();
    let state: HashMap<String, Value> =
        serde_json::from_value(serde_json::to_value(&first.actions.state_delta).unwrap()).unwrap();
    assert!(
        capture(turn, &state, &snapshot, current(WorkMode::Code), &[])
            .unwrap()
            .is_none()
    );
    let changed = capture(
        TurnId::new(),
        &state,
        &snapshot,
        current(WorkMode::Plan),
        &[],
    )
    .unwrap()
    .unwrap();
    assert_eq!(changed.actions.state_delta.len(), 1);
    assert!(changed.actions.state_delta.contains_key(POLICY));
    assert_eq!(state[SNAPSHOT], serde_json::to_value(&snapshot).unwrap());
    let text = changed
        .content()
        .unwrap()
        .parts
        .iter()
        .filter_map(|part| match part {
            adk_core::Part::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert!(text.contains("This is a planning turn"));
    assert!(!text.contains("Environment fixture"));
    assert!(!text.contains("Personal fixture"));
}

#[test]
fn retains_latest_context_through_recursive_compaction() {
    let guide = instructions::key("AGENTS.md");
    let mut initial = Event::new("first");
    initial
        .actions
        .state_delta
        .insert(POLICY.into(), json!("Old policy"));
    initial.actions.state_delta.insert(
        guide.clone(),
        json!({
            "path":"AGENTS.md","revision":"first","content":"Old guide content",
        }),
    );
    let mut changed = Event::new("next");
    changed
        .actions
        .state_delta
        .insert(POLICY.into(), json!("Current policy"));
    changed.actions.state_delta.insert(
        guide,
        json!({
            "path":"AGENTS.md","revision":"next","content":"Current guide content",
        }),
    );
    changed
        .actions
        .state_delta
        .insert(SNAPSHOT.into(), json!("Never copy declarations"));
    let mut text = "First summary".to_owned();
    let retained = retain(&[initial, changed], &mut text);
    assert_eq!(text.matches("Current policy").count(), 1);
    assert_eq!(text.matches("Current guide content").count(), 1);
    assert!(!text.contains("Old policy"));
    assert!(!text.contains("Old guide content"));
    assert!(!text.contains("Never copy declarations"));
    let mut summary = Event::new("summary");
    summary.actions.state_delta = retained;
    let mut text = "Second summary".to_owned();
    retain(&[summary], &mut text);
    assert_eq!(text.matches("Current policy").count(), 1);
    assert_eq!(text.matches("Current guide content").count(), 1);
}
