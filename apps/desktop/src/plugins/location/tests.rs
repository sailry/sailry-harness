use super::*;
use core::prelude::v1::test;
use sailry_client::Client;
use sailry_link::{Admission, Pending, Transport};
use sailry_protocol::{Fault, NodeId, Topic, plugin};

struct Offline;

impl Transport for Offline {
    fn target(&self) -> NodeId {
        NodeId([1; 32])
    }

    fn dispatch(&self, _: Request) -> Pending<'_, Result<Admission, Fault>> {
        panic!("location checks must not dispatch requests")
    }

    fn subscribe(
        &self,
        _: Topic,
    ) -> Pending<'_, Result<Box<dyn sailry_link::Subscription>, Fault>> {
        panic!("location checks must not create subscriptions")
    }
}

fn captured() -> (State, Arc<Host>) {
    let client = Arc::new(Client::new(Arc::new(Offline)));
    let runtime = Arc::new(
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap(),
    );
    let host = Host::new(
        client.clone(),
        plugin::Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: plugin::Reference {
                name: "worktrees".into(),
                digest: "fixture".into(),
                settings_revision: 0,
            },
            worktree: None,
            session: None,
        },
        runtime.handle().clone(),
        false,
        None,
    );
    let state = State {
        binding: Binding {
            client: client.clone(),
            defaults: client,
            runtime,
            project: None,
            worktree: None,
            host: "Fixture Node".into(),
            project_name: "".into(),
            branch: "".into(),
        },
        source: None,
        snapshot: None,
        connected: true,
        cursor: 3,
        changes: tokio::sync::watch::channel(json!({
            "node":crate::live::node_key(NodeId([1;32])),
            "worktree":null,
            "session":null,
            "cursor":"3",
        }))
        .0,
        known: BTreeSet::new(),
        confirmed: BTreeMap::new(),
        read: true,
        write: false,
        session_control: true,
        _observers: Vec::new(),
    };
    (state, host)
}

#[test]
fn distinguishes_unassigned_from_omitted() {
    let omitted: Selection = serde_json::from_value(json!({"projects":true})).unwrap();
    let unassigned: Selection = serde_json::from_value(json!({"project":null})).unwrap();
    let project = ProjectId::new();
    let assigned: Selection = serde_json::from_value(json!({"project":project})).unwrap();

    assert_eq!(omitted.project, None);
    assert_eq!(unassigned.project, Some(None));
    assert_eq!(assigned.project, Some(Some(project)));
    assert!(omitted.valid());
    assert!(unassigned.valid());
    assert!(assigned.valid());
}

#[test]
fn requires_one_selector() {
    let selectors = [
        json!({"project":null}),
        json!({"project":ProjectId::new()}),
        json!({"worktree":WorktreeId::new()}),
        json!({"projects":true}),
        json!({"add_project":true}),
        json!({"overview":true}),
    ];
    assert!(
        !serde_json::from_value::<Selection>(json!({}))
            .unwrap()
            .valid()
    );
    for (index, selector) in selectors.iter().enumerate() {
        assert!(
            serde_json::from_value::<Selection>(selector.clone())
                .unwrap()
                .valid()
        );
        for other in selectors.iter().skip(index + 1) {
            if selector.get("project").is_some() && other.get("project").is_some() {
                continue;
            }
            let mut combined = selector.as_object().unwrap().clone();
            combined.extend(other.as_object().unwrap().clone());
            assert!(
                !serde_json::from_value::<Selection>(Value::Object(combined))
                    .unwrap()
                    .valid()
            );
        }
    }
}

#[test]
fn preserves_worktree_modifiers() {
    let worktree = WorktreeId::new();
    let request = RequestId::new();
    let selection: Selection = serde_json::from_value(json!({
        "worktree":worktree,"fork":true,"request":request
    }))
    .unwrap();
    assert!(selection.valid());
    assert_eq!(selection.worktree, Some(worktree));
    assert_eq!(selection.request, Some(request));
    assert!(selection.fork);
}

#[test]
fn rejects_stale_cursor() {
    let (mut state, host) = captured();
    assert!(state.check(&host, Some("3")).is_ok());
    assert_eq!(
        state.check(&host, Some("2")),
        Err("location changed".into())
    );
    state.cursor += 1;
    assert_eq!(
        state.check(&host, Some("3")),
        Err("location changed".into())
    );
    assert!(state.check(&host, Some("4")).is_ok());
}

#[test]
fn rejects_changed_captured_scope() {
    let (state, host) = captured();
    let mut value = state.changes.borrow().clone();
    value["node"] = json!(crate::live::node_key(NodeId([2; 32])));
    state.changes.send_replace(value);
    assert_eq!(
        state.check(&host, Some("3")),
        Err("location is outside the captured view".into())
    );
}
