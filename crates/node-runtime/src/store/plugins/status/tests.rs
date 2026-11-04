use super::*;
use sailry_client::Client;
use sailry_protocol::{Command, Output, plugin, plugin::ui};
use serde_json::{Value, json};

mod recovery;

fn package() -> plugin::Info {
    serde_json::from_value(json!({
        "summary":{"name":"status-fixture","revision":1,"digest":"a".repeat(64),
            "settings_revision":0,"enabled":true,"version":"0.1.0","description":null},
        "origin":null,"icon":null,"settings":null,
        "extension":{"api_version":"v1","scope":"host",
            "host":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],
                "handlers":["read","invoke"],"read_only":["read"],"status":{"read":"read","invoke":"invoke"}},
            "ui":[
                {"id":"badge","slot":"status","kind":"button","label":{"label":"Badge"},"handler":"badge"},
                {"id":"other","slot":"status","kind":"button","label":{"label":"Other"},"handler":"other"},
                {"id":"settings","slot":"context","kind":"button","label":{"label":"Settings"},"handler":"settings"}
            ]}
    }))
    .unwrap()
}

fn state() -> Value {
    json!({"id":"badge","visible":true,"enabled":true,
        "segments":[{"label":{"label":"Ready","locales":{"zh-CN":"就绪"}},"tone":"success"}],
        "details":[{"label":{"label":"Items"},"value":"4"}]})
}

#[test]
fn preserves_storage_contracts() {
    for (scenario, check) in [
        (
            "validates_declared_states_and_omissions",
            (|scenario: &'static str| {
                let package = package();
                let states = evaluation::validate(&package, json!([state()])).unwrap();
                let entries = evaluation::entries(&package, states);
                assert_eq!(entries.len(), 2, "{scenario}");
                assert_eq!(entries[0].declaration.id, "badge", "{scenario}");
                assert!(entries[0].state.visible, "{scenario}");
                assert_eq!(entries[0].invoke.as_deref(), Some("invoke"), "{scenario}");
                assert_eq!(entries[0].state.details[0].value, "4", "{scenario}");
                assert_eq!(entries[1].declaration.id, "other", "{scenario}");
                assert!(!entries[1].state.visible, "{scenario}");
                assert!(!entries[1].state.enabled, "{scenario}");
            }) as fn(&'static str),
        ),
        (
            "rejects_invalid_and_oversized_states",
            (|scenario: &'static str| {
                let package = package();
                let state = state();
                let mut invalid = Vec::new();
                for (field, value) in [
                    ("id", json!("undeclared")),
                    ("id", json!("settings")),
                    ("reply_to", json!(1)),
                    ("value", json!(true)),
                    ("choices", json!([])),
                ] {
                    let mut changed = state.clone();
                    changed[field] = value;
                    invalid.push(json!([changed]));
                }
                invalid.push(json!([state, state]));
                invalid.push(json!({"states":[state]}));
                let mut oversized = state.clone();
                oversized["label"] = json!({"label":"x".repeat(ui::MAX_STATE_BYTES)});
                invalid.push(json!([oversized]));
                invalid.push(Value::Array(vec![state; ui::MAX_ENTRIES + 1]));
                for value in invalid {
                    assert_eq!(
                        evaluation::validate(&package, value).unwrap_err().code,
                        ErrorCode::InvalidRequest,
                        "{scenario}"
                    );
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}

#[tokio::test]
async fn shares_watchers_until_last_drop() {
    let directory = tempfile::tempdir().unwrap();
    let node = crate::Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let client = Client::new(node.local());
    let project = directory.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let Output::Project(project) = client
        .execute(client.prepare(Command::RegisterProject {
            name: "Status fixture".into(),
            path: project.to_str().unwrap().into(),
        }))
        .await
        .unwrap()
    else {
        panic!("project expected");
    };
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected");
    };
    let scope = Scope {
        worktree: Some(
            snapshot
                .worktrees
                .iter()
                .find(|worktree| worktree.project == Some(project.id))
                .unwrap()
                .id,
        ),
        session: None,
    };
    let mut first = node
        .ingress
        .status
        .subscribe(node.ingress.as_ref().clone(), scope)
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), first.next())
        .await
        .unwrap()
        .unwrap();
    let second = node
        .ingress
        .status
        .subscribe(node.ingress.as_ref().clone(), scope)
        .await
        .unwrap();
    let watcher = node.ingress.status.watchers.lock().unwrap()[&scope]
        .upgrade()
        .unwrap();
    let stop = watcher.stop.clone();
    assert_eq!(Arc::strong_count(&watcher), 3);
    drop(watcher);
    drop(first);
    assert!(!stop.is_cancelled());
    drop(second);
    assert!(stop.is_cancelled());
    assert!(
        node.ingress.status.watchers.lock().unwrap()[&scope]
            .upgrade()
            .is_none()
    );
    node.shutdown().await.unwrap();
}
