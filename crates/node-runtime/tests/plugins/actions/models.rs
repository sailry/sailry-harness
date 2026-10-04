use super::*;
use sailry_protocol::conversation::{Model, ModelApi, Provider};

#[tokio::test]
async fn requires_explicit_models() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let mut context = install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::ReadModels],
        )
        .await;
        context.worktree = None;
        let provider = Provider {
            options: None,
            id: ProviderId::new(),
            revision: 0,
            name: "Configured provider".into(),
            api: ModelApi::Responses,
            authentication: Authentication::ApiKey,
            endpoint: "http://127.0.0.1:1/v1".into(),
            enabled: true,
            credential: None,
            default_model: "explicit-model".into(),
            models: vec![Model {
                id: "explicit-model".into(),
                context: 4096,
                output: 2048,
                vision: false,
                tools: true,
                reasoning: true,
                web_search: false,
                generates: vec![],
                efforts: vec![Effort::Low, Effort::High],
                custom_efforts: false,
                default_effort: Effort::High,
            }],
        };
        let Output::Provider(provider) = execute(
            &client,
            Command::SaveProvider {
                provider,
                expected_revision: 0,
                secret: Some(Secret::new("isolated-model-key".into())),
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        let selected = format!("{}/explicit-model", provider.id);
        let Output::Snapshot(before) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        assert!(before.defaults.config.is_none());
        let resolve = |effort, config| {
            client
                .prepare(Command::ResolvePluginModel {
                    model: selected.clone(),
                    effort,
                    config,
                })
                .with_plugin(context.clone())
        };
        let pending = client.dispatch(resolve(None, None)).await.unwrap();
        assert!(!pending.receipt.durable);
        let Output::SessionConfig(config) = pending.completion.await.unwrap().unwrap() else {
            panic!("configuration expected")
        };
        assert_eq!(config.provider, provider.id);
        assert_eq!(config.model, "explicit-model");
        assert_eq!(config.effort, Effort::High);
        assert_eq!(config.permission, Permission::Ask);
        assert_eq!(config.mode, WorkMode::Code);
        assert_eq!(config.credential, provider.credential);
        assert_eq!(config.credential.as_ref().unwrap().node, node.id());
        assert!(
            !serde_json::to_string(&config)
                .unwrap()
                .contains("isolated-model-key")
        );
        let mut base = config.clone();
        base.mode = WorkMode::Plan;
        base.permission = Permission::Full;
        base.provider = ProviderId::new();
        base.model = "Previous model".into();
        base.credential = None;
        let Output::SessionConfig(changed) = client
            .execute(resolve(Some(Effort::Low), Some(base.clone())))
            .await
            .unwrap()
        else {
            panic!("configuration expected")
        };
        assert_eq!(changed.provider, config.provider);
        assert_eq!(changed.model, config.model);
        assert_eq!(changed.credential, config.credential);
        assert_eq!(changed.effort, Effort::Low);
        assert_eq!(changed.mode, base.mode);
        assert_eq!(changed.permission, base.permission);
        let Output::SessionConfig(retained) = client
            .execute(resolve(None, Some(changed.clone())))
            .await
            .unwrap()
        else {
            panic!("configuration expected")
        };
        assert_eq!(retained, changed);
        assert_eq!(
            client
                .execute(resolve(Some(Effort::Budget(1024)), None))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::ResolvePluginModel {
                            model: format!("{}/missing", provider.id),
                            effort: None,
                            config: None,
                        })
                        .with_plugin(context.clone())
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        let Output::Snapshot(after) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        assert_eq!(after.defaults, before.defaults);
        assert_eq!(after.sessions, before.sessions);
        assert_eq!(after.turns, before.turns);
        let mut denied = install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            1,
            &[Action::GenerateText],
        )
        .await;
        denied.worktree = None;
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::ResolvePluginModel {
                            model: selected,
                            effort: None,
                            config: None,
                        })
                        .with_plugin(denied)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        drop(client);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
