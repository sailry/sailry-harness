use super::*;
use serde_json::json;

async fn install(
    client: &Client,
    root: &std::path::Path,
    worktree: WorktreeId,
    name: &str,
    write: bool,
) -> plugin::Info {
    let directory = root.join(name);
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("plugin.json"), json!({
        "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":name,"version":"0.1.0",
        "extensions":{"dev.sailry.platform":{"api_version":"v1","scope":"host",
            "conversation_options":{"$schema":plugin::settings::SCHEMA,"type":"object","additionalProperties":false,
                "properties":{"mode":{"type":"string","default":"","enum":["","selected"]},"proactive":{"type":"boolean","default":false}}},
            "conversation_modes":[{"id":"selected","option":"mode","display":{"label":name},
                "permissions":{"write":write},"instructions":"Follow the selected workflow"}]}}
    }).to_string()).unwrap();
    let Output::Plugin(info) = client
        .execute(client.prepare(Command::InstallPlugin {
            worktree,
            path: name.into(),
            name: name.into(),
            expected_revision: 0,
            expected_digest: None,
        }))
        .await
        .unwrap()
    else {
        panic!("package expected")
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    info
}

async fn configure(client: &Client, session: &Session, config: SessionConfig) -> Session {
    let Output::Session(session) = client
        .execute(client.prepare(Command::SetSessionConfig {
            session: session.id,
            expected_revision: session.revision,
            config,
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    session
}

#[tokio::test]
async fn enforces_selection_and_frozen_authority() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let root = directory.path().join("source");
        std::fs::create_dir(&root).unwrap();
        client
            .execute(client.prepare(Command::RegisterProject {
                name: "Mode fixture".into(),
                path: root.to_str().unwrap().into(),
            }))
            .await
            .unwrap();
        let worktree = snapshot(&client).await.worktrees[0].id;
        let first = install(&client, &root, worktree, "first-mode", false).await;
        let second = install(&client, &root, worktree, "second-mode", true).await;
        let config: SessionConfig = serde_json::from_value(json!({
            "provider":ProviderId::new(),"model":"fixture","mode":"code","permission":"ask","credential":null,
            "options":{"first-mode":{"proactive":true},"second-mode":{"mode":""}}
        })).unwrap();
        let Output::Session(mut session) = client
            .execute(client.prepare(Command::CreateSession {
                project: None,
                worktree: None,
                config: Some(config.clone()),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        assert_eq!(session.config.mode, WorkMode::Code);
        let mut readonly = config;
        readonly
            .options
            .get_mut("first-mode")
            .unwrap()
            .insert("mode".into(), json!("selected"));
        assert_eq!(
            client
                .execute(client.prepare(Command::SetSessionConfig {
                    session: session.id,
                    expected_revision: session.revision,
                    config: readonly.clone(),
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        readonly.mode = WorkMode::Plan;
        let initial = session.revision;
        session = configure(&client, &session, readonly.clone()).await;
        assert_eq!(session.config.permission, Permission::Ask);
        assert_eq!(
            client
                .execute(client.prepare(Command::SetSessionConfig {
                    session: session.id,
                    expected_revision: initial,
                    config: readonly,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let Output::QueuedTurn(captured) = client
            .execute(client.prepare(Command::QueueTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Inspect changes".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        assert!(captured.plugins.contains(&first.summary.reference()));
        let mut ambiguous = session.config.clone();
        ambiguous
            .options
            .get_mut("second-mode")
            .unwrap()
            .insert("mode".into(), json!("selected"));
        assert_eq!(
            client
                .execute(client.prepare(Command::SetSessionConfig {
                    session: session.id,
                    expected_revision: session.revision,
                    config: ambiguous,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        let mut writable = session.config.clone();
        writable
            .options
            .get_mut("first-mode")
            .unwrap()
            .insert("mode".into(), json!(""));
        writable
            .options
            .get_mut("second-mode")
            .unwrap()
            .insert("mode".into(), json!("selected"));
        writable.mode = WorkMode::Code;
        session = configure(&client, &session, writable).await;
        assert_eq!(session.config.permission, Permission::Ask);
        client
            .execute(client.prepare(Command::SetPluginEnabled {
                name: second.summary.name.clone(),
                expected_revision: second.summary.revision,
                enabled: false,
            }))
            .await
            .unwrap();
        assert_eq!(
            client
                .execute(client.prepare(Command::QueueTurn {
                    session: session.id,
                    expected_revision: session.revision,
                    message: "Unavailable mode".into(),
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        let mut build = session.config.clone();
        build
            .options
            .get_mut("second-mode")
            .unwrap()
            .insert("mode".into(), json!(""));
        session = configure(&client, &session, build).await;
        assert_eq!(session.config.mode, WorkMode::Code);
        assert_eq!(
            session.config.options["first-mode"]["proactive"],
            json!(true)
        );
        assert_eq!(
            client
                .read_turn_capture(session.id, captured.id)
                .await
                .unwrap(),
            captured
        );
        assert_eq!(captured.config.mode, WorkMode::Plan);
        assert_eq!(
            captured.config.options["first-mode"]["mode"],
            json!("selected")
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
