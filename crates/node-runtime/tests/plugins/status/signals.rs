use super::*;

#[tokio::test]
async fn observes_runs_without_package_triggers() {
    let server = model_fixture::Server::markdown("Ready".into()).await;
    let fixture = Fixture::new().await;
    let model = completion::configure_empty(&fixture.client, server.endpoint.clone()).await;
    let provider = model.split('/').next().unwrap().parse().unwrap();
    let package = fixture
        .install("status-fixture", SOURCE, &[Change::Conversation], 0)
        .await;
    let mut session = fixture.session(None).await;
    session.config.provider = provider;
    let Output::Session(session) = execute(
        &fixture.client,
        Command::SetSessionConfig {
            session: session.id,
            expected_revision: session.revision,
            config: session.config,
        },
    )
    .await
    else {
        panic!("session expected")
    };
    let scope = Scope {
        worktree: Some(fixture.worktree),
        session: Some(session.id),
    };
    let mut feed = Feed::new(fixture.client.clone(), scope);
    feed.until(|view| fixture::detail(view, "Runs") == Some("0"))
        .await;
    execute(
        &fixture.client,
        Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Reply with Ready".into(),
        },
    )
    .await;
    let updated = feed
        .until(|view| fixture::detail(view, "Last run") == Some("completed"))
        .await;
    assert_eq!(fixture::detail(&updated, "Runs"), Some("1"));
    assert_eq!(updated.entries[0].package, package.summary.reference());
    assert!(updated.failures.is_empty());
    feed.close().await;
    fixture.close().await;
}

#[tokio::test]
async fn observes_repository_writes() {
    let fixture = Fixture::new().await;
    let root = fixture.root();
    let repository = git2::Repository::init(&root).unwrap();
    fs::write(root.join("tracked.txt"), "Original\n").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("tracked.txt")).unwrap();
    index.write().unwrap();
    let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
    let signature = git2::Signature::now("Status fixture", "status@example.invalid").unwrap();
    repository
        .commit(Some("HEAD"), &signature, &signature, "Initial", &tree, &[])
        .unwrap();
    drop(tree);
    drop(index);
    drop(repository);
    let source = r#"
            import {inspectGit} from 'sailry/sdk';
            export async function badge() {
                const repo=await inspectGit();
                const changed=repo.entries.find(entry=>entry.path==='tracked.txt');
                return [{id:'badge',visible:true,enabled:true,label:{label:String(changed?.diff.additions ?? 0)}}];
            }
            export function invoke() { return null; }
        "#;
    fixture
        .install("status-fixture", source, &[Change::Repository], 0)
        .await;
    let scope = Scope {
        worktree: Some(fixture.worktree),
        session: None,
    };
    let mut feed = Feed::new(fixture.client.clone(), scope);
    feed.until(|view| fixture::label(view) == Some("0")).await;
    fs::write(root.join("tracked.txt"), "Original\nAdded\n").unwrap();
    let changed = feed.until(|view| fixture::label(view) == Some("1")).await;
    assert!(changed.failures.is_empty());
    feed.close().await;
    fixture.close().await;
}

#[tokio::test]
async fn filters_assistants_and_refreshes_inventory() {
    let fixture = Fixture::new().await;
    let mut manifest = fixture.declaration("assistant-fixture", &[]);
    manifest["extensions"][plugin::NAMESPACE]["desktop"] = json!({
        "entry":"dev.sailry.platform/view.js","resources":["dev.sailry.platform/view.js"],
        "conversations":[{"id":"workspace","resource":"workspace","tools":[],"context":""}]
    });
    let assistant = fixture
        .install_with("assistant-fixture", SOURCE, manifest, 0)
        .await;
    let mut shared = fixture.declaration("shared-fixture", &[]);
    shared["extensions"][plugin::NAMESPACE]["desktop"] = json!({
        "ui_entry":"dev.sailry.platform/view.js","resources":["dev.sailry.platform/view.js"],"ui_shared":true
    });
    fixture
        .install_with("shared-fixture", SOURCE, shared, 0)
        .await;
    let other = fixture.install("other-fixture", SOURCE, &[], 0).await;
    let session = fixture
        .session(Some(plugin::conversation::Binding {
            package: assistant.summary.reference(),
            id: "workspace".into(),
        }))
        .await;
    let scope = Scope {
        worktree: Some(fixture.worktree),
        session: Some(session.id),
    };
    let mut feed = Feed::new(fixture.client.clone(), scope);
    let selected = feed
        .until(|view| {
            view.connected
                && view
                    .entries
                    .iter()
                    .filter(|entry| entry.state.visible)
                    .count()
                    == 2
        })
        .await;
    assert!(selected.entries.iter().all(|entry| matches!(
        entry.package.name.as_str(),
        "assistant-fixture" | "shared-fixture"
    )));
    assert_eq!(selected.entries.len(), 4);
    let mut manifest = fixture.declaration("other-fixture", &[]);
    manifest["version"] = json!("0.1.1");
    manifest["extensions"][plugin::NAMESPACE]["desktop"] = json!({
        "ui_entry":"dev.sailry.platform/view.js","resources":["dev.sailry.platform/view.js"],"ui_shared":true
    });
    let updated = fixture
        .install_with("other-fixture", SOURCE, manifest, other.summary.revision)
        .await;
    let all = feed
        .until(|view| {
            view.entries
                .iter()
                .any(|entry| entry.package.name == "other-fixture" && entry.state.visible)
        })
        .await;
    assert_eq!(all.entries.len(), 6);
    assert!(
        all.entries
            .iter()
            .filter(|entry| entry.package.name == "other-fixture")
            .all(|entry| entry.package == updated.summary.reference())
    );
    feed.close().await;
    fixture.close().await;
}
