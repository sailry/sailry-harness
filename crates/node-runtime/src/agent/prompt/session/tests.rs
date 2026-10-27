use super::*;
use adk_core::{AdkError, Tool};
use async_trait::async_trait;
use sailry_protocol::{Permission, WorkMode};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture {
    description: String,
    parameters: Value,
    captures: Arc<AtomicUsize>,
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

    fn declaration(&self) -> Value {
        self.captures.fetch_add(1, Ordering::Relaxed);
        json!({"name":self.name(),"description":self.description(),"parameters":self.parameters})
    }

    async fn execute(&self, _: Arc<dyn ToolContext>, _: Value) -> adk_core::Result<Value> {
        Err(AdkError::tool("fixture tools do not execute"))
    }
}

fn tools(description: &str) -> Vec<Registration> {
    vec![Registration::managed(Arc::new(Fixture {
        description: description.into(),
        parameters: json!({"type":"object"}),
        captures: Default::default(),
    }))]
}

fn captured(
    description: &str,
    parameters: Value,
    captures: &Arc<AtomicUsize>,
) -> Vec<Registration> {
    let tool = Arc::new(Fixture {
        description: description.into(),
        parameters,
        captures: captures.clone(),
    });
    let mut registration = Registration::managed(tool.clone());
    registration.capture((&tool.parameters, Option::<Value>::None));
    vec![registration]
}

#[test]
fn reuses_exact_inputs_across_restores() {
    let captures = Arc::new(AtomicUsize::new(0));
    let schema = json!({"type":"object","properties":{"path":{"type":"string"}}});
    let mut snapshot = Snapshot::default();
    let first = snapshot.declarations(&captured("Read a file", schema.clone(), &captures));
    assert_eq!(captures.load(Ordering::Relaxed), 1);
    let state = serde_json::to_value(&snapshot).unwrap();
    let mut snapshot: Snapshot = serde_json::from_value(state.clone()).unwrap();
    let unchanged = snapshot.declarations(&captured("Read a file", schema, &captures));
    assert_eq!(captures.load(Ordering::Relaxed), 1);
    assert_eq!(project(first), project(unchanged));
    assert_eq!(serde_json::to_value(&snapshot).unwrap(), state);
    let changed = snapshot.declarations(&captured(
        "Read a file",
        json!({"type":"object","properties":{"path":{"type":"string"},"limit":{"type":"integer"}}}),
        &captures,
    ));
    assert_eq!(captures.load(Ordering::Relaxed), 2);
    assert!(
        project(changed)["fixture"]["parameters"]["properties"]
            .get("limit")
            .is_some()
    );
    let changed = snapshot.declarations(&captured(
        "Read the selected file",
        json!({"type":"object"}),
        &captures,
    ));
    assert_eq!(captures.load(Ordering::Relaxed), 3);
    assert_eq!(
        project(changed)["fixture"]["description"],
        "Read the selected file"
    );
    assert!(snapshot.declarations(&[]).is_empty());
    snapshot.declarations(&captured(
        "Read a file",
        json!({"type":"object"}),
        &captures,
    ));
    assert_eq!(captures.load(Ordering::Relaxed), 4);
}

#[test]
fn reuses_native_and_managed_definitions() {
    let captures = Arc::new(AtomicUsize::new(0));
    let tool: Arc<dyn Tool> = Arc::new(Fixture {
        description: "Current definition".into(),
        parameters: json!({"type":"object"}),
        captures: captures.clone(),
    });
    let mut snapshot = Snapshot::default();
    snapshot.declarations(&[Registration::managed(tool.clone())]);
    snapshot.declarations(&[Registration::managed(tool)]);
    assert_eq!(captures.load(Ordering::Relaxed), 1);
    let changed = Arc::new(Fixture {
        description: "Changed definition".into(),
        parameters: json!({"type":"object","properties":{"path":{"type":"string"}}}),
        captures: captures.clone(),
    });
    let declarations = snapshot.declarations(&[Registration::managed(changed)]);
    assert_eq!(captures.load(Ordering::Relaxed), 2);
    assert_eq!(
        project(declarations)["fixture"]["description"],
        "Changed definition"
    );
}

#[test]
fn restores_discovered_declarations() {
    let mut snapshot = Snapshot::default();
    let mut tools = tools("Deferred capability");
    tools[0].deferred = true;
    let declarations = snapshot.declarations(&tools);
    assert!(project(declarations.clone()).is_empty());
    let loaded: BTreeSet<_> = ["fixture".into()].into();
    let expected = project_with_loaded(declarations, &loaded);
    assert_eq!(expected.len(), 1);
    let mut catalog = Event::new("turn");
    catalog
        .actions
        .state_delta
        .insert(SNAPSHOT.into(), serde_json::to_value(&snapshot).unwrap());
    let mut discovered = Event::new("turn");
    discovered
        .actions
        .state_delta
        .insert(LOADED_TOOLS.into(), json!(loaded));
    assert_eq!(
        super::declarations(&[catalog, discovered]).unwrap(),
        expected
    );
    let state = [(LOADED_TOOLS.into(), json!(["fixture"]))].into();
    let (state, _) = compose(state, &[]);
    assert_eq!(state[LOADED_TOOLS], json!(["fixture"]));
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
    let restored = resumed.declarations(&tools("First description"));
    assert_eq!(
        serde_json::to_value(&restored).unwrap(),
        serde_json::to_value(&declarations).unwrap()
    );
    assert_eq!(serde_json::to_value(resumed).unwrap(), captured);

    let projected = project(declarations);
    let expected = json!({
        "name":"fixture","description":"First description","parameters":{"type":"object"}
    });
    assert_eq!(projected.len(), 1);
    assert_eq!(projected["fixture"], expected);
    assert_eq!(project(restored), projected);
    assert!(!instruction.contains("This turn uses Ask permission"));
    assert!(!instruction.contains("This is a planning turn"));
}

#[test]
fn updates_the_captured_catalog() {
    let mut snapshot = Snapshot::default();
    let instruction = snapshot.instruction(super::super::baseline().into());
    let first = snapshot.declarations(&tools("First description"));
    let captured = serde_json::to_value(&snapshot).unwrap();
    let mut resumed: Snapshot = serde_json::from_value(captured).unwrap();
    let changed = resumed.declarations(&tools("Changed description"));
    assert_eq!(
        resumed.instruction("Changed instructions".into()),
        instruction
    );
    assert_eq!(
        project(first)["fixture"]["description"],
        "First description"
    );
    assert_eq!(
        project(changed)["fixture"]["description"],
        "Changed description"
    );
    let empty = resumed.declarations(&[]);
    assert!(project(empty).is_empty());
    let persisted = serde_json::to_value(&resumed).unwrap();
    assert_eq!(persisted["tools"], json!({}));
    let mut restored: Snapshot = serde_json::from_value(persisted).unwrap();
    assert!(restored.declarations(&[]).is_empty());
    assert_eq!(restored.instruction("Changed again".into()), instruction);
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
fn defers_temporarily_mounted_tools() {
    let turn = TurnId::new();
    let mut snapshot = Snapshot::default();
    snapshot.declarations(&tools("Initially selected tool"));
    let first = capture(
        turn,
        &HashMap::new(),
        &snapshot,
        current(WorkMode::Code),
        &[],
    )
    .unwrap()
    .unwrap();
    let state: HashMap<String, Value> = first.actions.state_delta.into_iter().collect();
    assert!(loaded(state.get(LOADED_TOOLS)).is_empty());
    let mut deferred = tools("Initially selected tool");
    deferred[0].deferred = true;
    let declarations = snapshot.declarations(&deferred);
    assert!(project_with_loaded(declarations, &loaded(state.get(LOADED_TOOLS))).is_empty());
    let changed = capture(turn, &state, &snapshot, current(WorkMode::Code), &[])
        .unwrap()
        .unwrap();
    assert!(!changed.actions.state_delta.contains_key(LOADED_TOOLS));
    assert!(!changed.actions.state_delta.contains_key(POLICY));
}

#[test]
fn restores_latest_selected_capability() {
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
    assert!(!text.contains("Earlier selected skill"));
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
    let mut snapshot = Snapshot::default();
    snapshot.instruction("Never copy declarations".into());
    let expected = project(snapshot.declarations(&tools("Latest exact schema")));
    changed
        .actions
        .state_delta
        .insert(SNAPSHOT.into(), serde_json::to_value(snapshot).unwrap());
    let events = [initial, changed];
    assert_eq!(declarations(&events).unwrap(), expected);
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
    // The materialized event is a read projection, not a stored summary action.
    let mut projected = Event::new("summary");
    projected.actions.state_delta = state;
    assert_eq!(
        declarations(std::slice::from_ref(&projected)).unwrap(),
        expected
    );
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
