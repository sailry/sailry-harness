use super::*;
use std::{fs, time::Duration};

async fn ready(subscription: &mut dyn Subscription) -> Snapshot {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let Update::Status(snapshot) = subscription.next().await.unwrap() else {
                panic!("Status snapshot expected");
            };
            if snapshot.entries.iter().any(|entry| entry.state.visible) {
                return snapshot;
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn replaces_terminal_watchers_for_new_subscribers() {
    let directory = tempfile::tempdir().unwrap();
    let node = crate::Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let client = Client::new(node.local());
    let project = directory.path().join("project");
    let package = project.join("package");
    fs::create_dir_all(package.join(plugin::NAMESPACE)).unwrap();
    fs::write(
        package.join("plugin.json"),
        json!({
            "$schema":plugin::schema::AGENT_PLUGINS_ID,"name":"status-fixture","version":"0.1.0",
            "extensions":{"dev.sailry.platform":{
                "api_version":"v1","scope":"host",
                "host":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],
                    "handlers":["badge"],"read_only":["badge"],"status":{"read":"badge","changes":["repository"]}},
                "ui":[{"id":"badge","slot":"status","kind":"button","label":{"label":"Badge"},"handler":"badge"}]
            }}
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        package.join("dev.sailry.platform/main.js"),
        "export function badge() { return [{id:'badge',visible:true,enabled:true,label:{label:'Ready'}}]; }",
    )
    .unwrap();
    client
        .execute(client.prepare(Command::RegisterProject {
            name: "Status recovery".into(),
            path: project.to_str().unwrap().into(),
        }))
        .await
        .unwrap();
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("Node snapshot expected");
    };
    let worktree = snapshot.worktrees[0].id;
    let Output::Plugin(package) = client
        .execute(client.prepare(Command::InstallPlugin {
            worktree,
            path: "package".into(),
            name: "status-fixture".into(),
            expected_revision: 0,
            expected_digest: None,
        }))
        .await
        .unwrap()
    else {
        panic!("plugin information expected");
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    let scope = Scope {
        worktree: Some(worktree),
        session: None,
    };
    let mut first = node
        .ingress
        .status
        .subscribe(node.ingress.as_ref().clone(), scope)
        .await
        .unwrap();
    let before = ready(first.as_mut()).await;
    let previous = node.ingress.status.watchers.lock().unwrap()[&scope]
        .upgrade()
        .unwrap();
    let root = node.ingress.worktree_root(worktree).await.unwrap();
    // Fail the actual repository subscription while leaving its registered root valid.
    node.ingress.watches.remove(&root);
    let error = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Err(error) = first.next().await {
                return error;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(error.code, ErrorCode::Unavailable);
    let mut recovered = node
        .ingress
        .status
        .subscribe(node.ingress.as_ref().clone(), scope)
        .await
        .unwrap();
    let after = ready(recovered.as_mut()).await;
    assert_eq!(after, before);
    let current = node.ingress.status.watchers.lock().unwrap()[&scope]
        .upgrade()
        .unwrap();
    assert!(!Arc::ptr_eq(&previous, &current));
    assert_eq!(first.next().await.unwrap_err(), error);
    drop(first);
    drop(previous);
    assert!(!current.stop.is_cancelled());
    drop(current);
    drop(recovered);
    node.shutdown().await.unwrap();
}
