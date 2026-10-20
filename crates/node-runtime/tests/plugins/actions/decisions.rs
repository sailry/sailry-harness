use super::*;
use sailry_protocol::{decision, models};
use std::collections::BTreeMap;

fn input(model: Option<String>) -> decision::Request {
    decision::Request {
        model,
        state: "Save is visible".into(),
        questions: BTreeMap::from([(
            "save".into(),
            decision::Question::Noul {
                instructions: "Is Save visible?".into(),
                criteria: BTreeMap::new(),
            },
        )]),
    }
}

#[tokio::test]
async fn requires_permission_and_bounds_both_links() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let mut denied = install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::ReadModels],
        )
        .await;
        denied.worktree = None;
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::Decide(input(None)))
                        .with_plugin(denied)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied,
        );
        let mut permitted = install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            1,
            &[Action::RunDecisions],
        )
        .await;
        permitted.worktree = None;
        let mut invalid = input(None);
        invalid.state = "x".repeat(decision::MAX_STATE_BYTES + 1);
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::Decide(invalid))
                        .with_plugin(permitted.clone())
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest,
        );
        let request = client
            .prepare(Command::Decide(input(None)))
            .with_plugin(permitted);
        assert!(!request.command.durable());
        let error = client.execute(request).await.unwrap_err();
        assert!(
            matches!(
                error.code,
                ErrorCode::NotConfigured | ErrorCode::Unavailable
            ),
            "{error}"
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
#[ignore = "requires the pinned Julia GGUF and packaged CPU runtime"]
async fn installs_and_decides_through_both_links() {
    let cached = std::env::var_os("SAILRY_DECISION_MODEL_CACHE")
        .expect("SAILRY_DECISION_MODEL_CACHE must name the pinned Julia GGUF");
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let mut context = install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::RunDecisions],
        )
        .await;
        context.worktree = None;
        let source = models::Source {
            hub: models::Hub::HuggingFace,
            repository: "ggml-org/Julia-1-GGUF".into(),
            revision: "1e6b48a6983d7467f8eedc2cb0d21267b5fc052d".into(),
        };
        let definition = models::Definition {
            source: source.clone(),
            name: "Julia-1".into(),
            license: "Apache-2.0".into(),
            format: models::Format::Decision {},
            query_prefix: None,
            document_prefix: None,
            assets: vec![models::Asset {
                source,
                path: "Julia-1-Q8_0.gguf".into(),
                name: "model.gguf".into(),
                bytes: 168_166_496,
                sha256: "1ea6a7e87156eeeda88cb7a36a61265b37ba7b993897b7289b99aea5b5e47069".into(),
            }],
        };
        let Output::LocalModel { id } =
            execute(&client, Command::RegisterLocalModel { definition }).await
        else {
            panic!("registered decision model expected");
        };
        let assets = directory.path().join("node/models/decision").join(&id);
        fs::create_dir_all(&assets).unwrap();
        fs::copy(&cached, assets.join("model.gguf")).unwrap();
        execute(&client, Command::InstallLocalModel { id: id.clone() }).await;
        let Output::LocalModels(catalog) = execute(
            &client,
            Command::ReadLocalModels {
                kind: Some(models::Kind::Decision),
            },
        )
        .await
        else {
            panic!("decision catalog expected");
        };
        assert!(catalog.availability[&models::Kind::Decision].available());
        assert_eq!(catalog.models.len(), 1);
        assert_eq!(catalog.models[0].id, id);
        assert_eq!(catalog.models[0].state, models::State::Installed);
        let Output::LocalModelSettings(mut settings) =
            execute(&client, Command::ReadLocalModelSettings).await
        else {
            panic!("local settings expected");
        };
        assert_eq!(settings.selected(models::Kind::Decision), None);
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::Decide(input(None)))
                        .with_plugin(context.clone())
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured,
        );
        let mut oversized = input(Some(id.clone()));
        oversized.state = "x ".repeat(decision::MAX_STATE_BYTES / 2);
        oversized.questions.insert(
            "save".into(),
            decision::Question::Noul {
                instructions: "x ".repeat(decision::MAX_INSTRUCTIONS_BYTES / 2),
                criteria: BTreeMap::new(),
            },
        );
        oversized.validate().unwrap();
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::Decide(oversized))
                        .with_plugin(context.clone())
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest,
        );
        let request = client
            .prepare(Command::Decide(input(Some(id.clone()))))
            .with_plugin(context.clone());
        let admission = client.dispatch(request).await.unwrap();
        assert!(!admission.receipt.durable);
        let Output::Decision(explicit) = admission.completion.await.unwrap().unwrap() else {
            panic!("decision expected");
        };
        settings.choices.insert(
            models::Kind::Decision,
            models::Choice {
                enabled: true,
                model: Some(id.clone()),
            },
        );
        execute(&client, Command::SaveLocalModelSettings(settings)).await;
        assert_eq!(
            execute(&client, Command::Decide(input(None))).await,
            Output::Decision(explicit.clone())
        );
        let peer = Client::new(if remote {
            node.local()
        } else {
            controller.handle().remote(node.link().address())
        });
        assert_eq!(
            execute(&peer, Command::Decide(input(None))).await,
            Output::Decision(explicit.clone())
        );
        assert!(explicit.usage.input_tokens > 0);
        assert_eq!(explicit.usage.output_tokens, 0);
        assert!(
            matches!(&explicit.answers["save"], decision::Answer::Noul { noul } if noul.is_finite() && (0.0..=1.0).contains(noul))
        );
        let Output::LocalModelSettings(mut disabled) =
            execute(&client, Command::ReadLocalModelSettings).await
        else {
            panic!("local settings expected");
        };
        disabled
            .choices
            .get_mut(&models::Kind::Decision)
            .unwrap()
            .enabled = false;
        execute(&client, Command::SaveLocalModelSettings(disabled)).await;
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::Decide(input(Some(id.clone()))))
                        .with_plugin(context)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured,
        );
        execute(&client, Command::DeleteLocalModel { id }).await;
        assert!(!assets.exists());
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
