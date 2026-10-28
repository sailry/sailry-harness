use super::*;
use adk_core::{AdkError, Tool};
use async_trait::async_trait;
use sailry_protocol::{Permission, WorkMode};

struct Fixture {
    description: String,
    parameters: Value,
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
        Some(self.parameters.clone())
    }

    async fn execute(&self, _: Arc<dyn ToolContext>, _: Value) -> adk_core::Result<Value> {
        Err(AdkError::tool("fixture tools do not execute"))
    }
}

fn tools(description: &str) -> Vec<Registration> {
    vec![Registration::managed(Arc::new(Fixture {
        description: description.into(),
        parameters: json!({"type":"object"}),
    }))]
}

#[test]
fn uses_current_definitions() {
    let first = project(declarations(&tools("First description")), &BTreeSet::new());
    let changed = project(
        declarations(&tools("Changed description")),
        &BTreeSet::new(),
    );
    assert_eq!(first["fixture"]["description"], "First description");
    assert_eq!(changed["fixture"]["description"], "Changed description");
    assert!(project(declarations(&[]), &["fixture".into()].into()).is_empty());
}

#[test]
fn intersects_discovery_with_current_tools() {
    let mut tools = tools("Deferred capability");
    tools[0].deferred = true;
    assert!(project(declarations(&tools), &BTreeSet::new()).is_empty());
    let loaded: BTreeSet<_> = ["fixture".into(), "removed".into()].into();
    let selected = project(declarations(&tools), &loaded);
    assert_eq!(selected.len(), 1);
    assert!(selected.contains_key("fixture"));
    let state = [(LOADED_TOOLS.into(), json!(["fixture"]))].into();
    let (state, _) = compose(state, &[]);
    assert!(!state.contains_key(LOADED_TOOLS));
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
fn appends_only_changed_runtime_context() {
    let turn = TurnId::new();
    let first = capture(turn, &HashMap::new(), current(WorkMode::Code), &[])
        .unwrap()
        .unwrap();
    let state: HashMap<String, Value> =
        serde_json::from_value(serde_json::to_value(&first.actions.state_delta).unwrap()).unwrap();
    assert!(
        capture(turn, &state, current(WorkMode::Code), &[])
            .unwrap()
            .is_none()
    );
    let changed = capture(TurnId::new(), &state, current(WorkMode::Plan), &[])
        .unwrap()
        .unwrap();
    assert_eq!(changed.actions.state_delta.len(), 1);
    assert!(changed.actions.state_delta.contains_key(POLICY));
    assert!(!state.contains_key("sailry.context.declarations"));
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
fn defers_temporarily_mounted_tools() {
    let turn = TurnId::new();
    let first = capture(turn, &HashMap::new(), current(WorkMode::Code), &[])
        .unwrap()
        .unwrap();
    let state: HashMap<String, Value> = first.actions.state_delta.into_iter().collect();
    assert!(loaded(state.get(LOADED_TOOLS)).is_empty());
    let mut deferred = tools("Initially selected tool");
    deferred[0].deferred = true;
    let declarations = declarations(&deferred);
    assert!(project(declarations, &loaded(state.get(LOADED_TOOLS))).is_empty());
    assert!(
        capture(turn, &state, current(WorkMode::Code), &[])
            .unwrap()
            .is_none()
    );
}

#[test]
fn preserves_capability_sources() {
    let (_, parts) = compose(
        HashMap::new(),
        &[
            Part::Text {
                text: "Earlier selected skill".into(),
            },
            Part::Text {
                text: "Latest selected skill".into(),
            },
        ],
    );
    let text = serde_json::to_string(&parts).unwrap();
    assert!(text.contains("Latest selected skill"));
    assert!(text.contains("Earlier selected skill"));
}

#[test]
fn materializes_exact_captured_context() {
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
    changed.actions.state_delta.insert(
        "sailry.context.declarations".into(),
        json!({"instruction":"Never copy declarations", "tools":{"stale":{"value":{"name":"stale"}}}}),
    );
    let events = [initial, changed];
    let captured = events
        .iter()
        .flat_map(|event| event.actions.state_delta.clone())
        .collect();
    let (state, first) = compose(captured, &[]);
    let text = serde_json::to_string(&first).unwrap();
    assert_eq!(text.matches("Current policy").count(), 1);
    assert_eq!(text.matches("Current guide content").count(), 1);
    assert!(!text.contains("Old policy"));
    assert!(!text.contains("Old guide content"));
    assert!(!text.contains("Never copy declarations"));
    assert!(!state.contains_key("sailry.context.declarations"));
    let restored: Vec<Event> =
        serde_json::from_slice(&serde_json::to_vec(&events).unwrap()).unwrap();
    let captured = restored
        .iter()
        .flat_map(|event| event.actions.state_delta.clone())
        .collect();
    let (_, second) = compose(captured, &[]);
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
    let text = serde_json::to_string(&second).unwrap();
    assert_eq!(text.matches("Current policy").count(), 1);
    assert_eq!(text.matches("Current guide content").count(), 1);
}
