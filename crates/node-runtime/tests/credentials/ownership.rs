use super::*;
use sailry_link::{Link, NetworkScope};
use sailry_protocol::conversation::{ModelApi, Provider};

#[tokio::test]
async fn retains_replaced_and_removed_configuration() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = Node::start_empty(&profile).await.unwrap();
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
        let provider = Provider {
            request_max_retries: None,
            stream_max_retries: None,
            preset: None,
            binding: None,
            consent: None,
            oauth: None,
            id: ProviderId::new(),
            revision: 0,
            name: "Owner fixture".into(),
            api: ModelApi::ChatCompletions,
            authentication: Authentication::ApiKey,
            endpoint: "https://fixture.invalid/v1".into(),
            options: None,
            enabled: true,
            models: Vec::new(),
            default_model: "fixture".into(),
            credential: None,
        };
        let Output::Provider(provider) = client
            .execute(client.prepare(Command::SaveProvider {
                provider,
                expected_revision: 0,
                secret: Some(Secret::new("original-inline-key".into())),
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        let original = provider.credential.clone().unwrap();
        let Output::Session(session) = client
            .execute(client.prepare(Command::CreateSession {
                worktree: None,
                project: None,
                config: Some(SessionConfig {
                    features: Default::default(),
                    assistant: None,
                    resource: None,
                    provider: provider.id,
                    model: "fixture".into(),
                    effort: Effort::Default,
                    mode: WorkMode::Code,
                    options: Default::default(),
                    permission: Permission::Ask,
                    credential: Some(original.clone()),
                }),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let Output::Provider(provider) = client
            .execute(client.prepare(Command::SaveProvider {
                expected_revision: provider.revision,
                provider,
                secret: Some(Secret::new("replacement-inline-key".into())),
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        let replacement = provider.credential.clone().unwrap();
        assert_ne!(original.id, replacement.id);
        let Output::Session(current) = client
            .execute(client.prepare(Command::ReadSession {
                session: session.id,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        assert_eq!(current.config.credential, Some(original.clone()));
        client
            .execute(client.prepare(Command::RemoveProvider {
                provider: provider.id,
                expected_revision: provider.revision,
            }))
            .await
            .unwrap();
        assert_eq!(
            client
                .execute(client.prepare(Command::ListProviders))
                .await
                .unwrap(),
            Output::Providers(Vec::new())
        );

        let body: Vec<u8> = {
            let sql = ("SELECT body FROM providers WHERE id=?1").to_owned();
            let parameters = turso::params![provider.id.to_string()];
            node.with_storage(move |db| {
                Box::pin(async move {
                    let row = db
                        .prepare(sql)
                        .await
                        .map_err(crate::database_fixture::fault)?
                        .query_row(parameters)
                        .await
                        .map_err(crate::database_fixture::fault)?;
                    let value: turso::Result<_> = row.get(0);
                    value.map_err(crate::database_fixture::fault)
                })
            })
            .await
        }
        .unwrap();
        let stored: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(stored["listed"], false);
        assert_eq!(stored["provider"]["id"], provider.id.to_string());
        assert_eq!(
            stored["authorizations"][original.id.to_string()]["key"],
            "original-inline-key"
        );
        assert_eq!(
            stored["authorizations"][replacement.id.to_string()]["key"],
            "replacement-inline-key"
        );

        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();

        let restored = Node::start_empty(&profile).await.unwrap();
        for (reference, key) in [
            (original, "original-inline-key"),
            (replacement, "replacement-inline-key"),
        ] {
            assert_eq!(
                restored
                    .resolve_credential(reference, provider.id)
                    .await
                    .unwrap()
                    .expose(),
                key
            );
        }
        restored.shutdown().await.unwrap();
    }
}
