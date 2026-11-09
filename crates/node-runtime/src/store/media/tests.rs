use super::*;
use sailry_protocol::conversation::ModelApi;

fn fixture() -> (Provider, SessionConfig) {
    let provider = Provider {
        request_max_retries: None,
        stream_max_retries: None,
        preset: None,
        binding: None,
        consent: None,
        id: ProviderId::new(),
        revision: 1,
        name: "Captured media".into(),
        api: ModelApi::Responses,
        authentication: Authentication::ApiKey,
        endpoint: "https://example.test/v1".into(),
        enabled: true,
        options: None,
        oauth: None,
        default_model: "image-model".into(),
        credential: Some(CredentialRef {
            node: NodeId([17; 32]),
            id: CredentialId::new(),
        }),
        models: vec![sailry_protocol::conversation::Model {
            id: "image-model".into(),
            display_name: None,
            context: 16000,
            output: 4096,
            vision: false,
            tools: false,
            reasoning: false,
            generates: vec![],
            efforts: vec![Effort::Low, Effort::High],
            capabilities: None,
            custom_efforts: false,
            compaction_tokens: None,
            default_effort: Effort::Low,
        }],
    };
    let config = SessionConfig {
        features: Default::default(),
        assistant: None,
        resource: None,
        provider: provider.id,
        model: "image-model".into(),
        effort: Effort::High,
        mode: WorkMode::Code,
        options: Default::default(),
        permission: Permission::Ask,
        credential: Some(CredentialRef {
            node: NodeId([17; 32]),
            id: CredentialId::new(),
        }),
    };
    (provider, config)
}

#[test]
fn preserves_storage_contracts() {
    for (scenario, check) in [
        (
            "plans_admitted_configuration",
            (|scenario: &'static str| {
                let (provider, config) = fixture();
                let models = for_turn(Snapshot::new(), Some(&provider), &config);
                assert_eq!(models.len(), 3, "{scenario}");
                for kind in [Kind::Vision, Kind::Image, Kind::Video] {
                    let plan = &models[&kind];
                    let model = plan.primary.as_ref().unwrap();
                    assert_eq!(model.provider.id, config.provider, "{scenario}");
                    assert_eq!(model.model, config.model, "{scenario}");
                    assert_eq!(model.provider.credential, config.credential, "{scenario}");
                    assert_eq!(model.effort, Some(config.effort), "{scenario}");
                    assert!(plan.fallback.is_none(), "{scenario}");
                }
                let mut changed = config.clone();
                changed.model = "missing-model".into();
                let models = for_turn(Snapshot::new(), Some(&provider), &changed);
                assert_eq!(models.len(), 3, "{scenario}");
                assert!(
                    models.values().all(|plan| {
                        plan.primary.as_ref().unwrap().model == "missing-model"
                            && plan.fallback.is_none()
                    }),
                    "{scenario}"
                );
                assert!(
                    for_turn(Snapshot::new(), None, &config).is_empty(),
                    "{scenario}"
                );
            }) as fn(&'static str),
        ),
        (
            "preserves_disabled_and_frozen_fallback",
            (|scenario: &'static str| {
                let (provider, config) = fixture();
                let snapshot = [
                    (Kind::Image, None),
                    (
                        Kind::Vision,
                        Some(Model {
                            provider: provider.clone(),
                            model: config.model.clone(),
                            effort: None,
                        }),
                    ),
                ]
                .into_iter()
                .collect();
                let models = for_turn(snapshot, Some(&provider), &config);
                assert_eq!(models.len(), 2, "{scenario}");
                assert!(!models.contains_key(&Kind::Image), "{scenario}");
                let vision = &models[&Kind::Vision];
                assert_eq!(
                    vision.primary.as_ref().unwrap().provider.credential,
                    config.credential,
                    "{scenario}"
                );
                let fallback = vision.fallback.as_ref().unwrap();
                assert_eq!(
                    fallback.provider.credential, provider.credential,
                    "{scenario}"
                );
                assert_ne!(
                    fallback.provider.credential, config.credential,
                    "{scenario}"
                );
                assert_eq!(fallback.effort, None, "{scenario}");
                assert!(models[&Kind::Video].primary.is_some(), "{scenario}");
                assert!(models[&Kind::Video].fallback.is_none(), "{scenario}");
            }),
        ),
        (
            "keeps_fallback_without_primary",
            (|scenario: &'static str| {
                let (provider, config) = fixture();
                let models = for_turn(
                    [
                        (
                            Kind::Vision,
                            Some(Model {
                                provider: provider.clone(),
                                model: config.model.clone(),
                                effort: None,
                            }),
                        ),
                        (Kind::Image, None),
                    ]
                    .into(),
                    None,
                    &config,
                );
                assert_eq!(models.len(), 1, "{scenario}");
                let plan = &models[&Kind::Vision];
                assert!(plan.primary.is_none(), "{scenario}");
                assert_eq!(
                    plan.fallback.as_ref().unwrap().provider.credential,
                    provider.credential,
                    "{scenario}"
                );
                assert!(!models.contains_key(&Kind::Video), "{scenario}");
            }),
        ),
        (
            "keeps_runtime_effort_out_of_snapshots",
            (|scenario: &'static str| {
                let (provider, config) = fixture();
                let model = for_turn(Snapshot::new(), Some(&provider), &config)
                    .remove(&Kind::Image)
                    .unwrap()
                    .primary
                    .unwrap();
                let body = serde_json::to_value(&model).unwrap();
                assert!(body.get("effort").is_none(), "{scenario}");
                let restored: Model = serde_json::from_value(body).unwrap();
                assert_eq!(restored.effort, None, "{scenario}");
                assert_eq!(
                    restored.provider.credential, config.credential,
                    "{scenario}"
                );
                let snapshot: Snapshot =
                    [(Kind::Image, Some(restored)), (Kind::Video, None)].into();
                let body = serde_json::to_value(&snapshot).unwrap();
                assert!(body["image"].get("effort").is_none(), "{scenario}");
                assert!(body["image"].get("primary").is_none(), "{scenario}");
                assert!(body["image"].get("fallback").is_none(), "{scenario}");
                assert_eq!(body["video"], serde_json::Value::Null, "{scenario}");
                let restored: Snapshot = serde_json::from_value(body).unwrap();
                assert_eq!(
                    restored[&Kind::Image].as_ref().unwrap().provider.credential,
                    config.credential,
                    "{scenario}"
                );
                assert!(restored[&Kind::Video].is_none(), "{scenario}");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}

#[tokio::test]
async fn captures_login_override_options() {
    let (mut provider, _) = fixture();
    let directory = tempfile::tempdir().unwrap();
    let database = crate::store::database::Database::open(
        &crate::store::tests::profile(&directory).join("node.turso"),
        NodeId([18; 32]),
        None,
    )
    .await
    .unwrap();
    provider.authentication = Authentication::ChatGpt;
    provider.endpoint = crate::providers::login::CHATGPT_ENDPOINT.into();
    provider.credential = None;
    provider.models[0].vision = false;
    provider.models[0].tools = false;
    let binding = media::Binding {
        provider: provider.id,
        model: "image-model".into(),
    };
    crate::store::agent::providers::put(&database.connection, database.node, &provider, 0)
        .await
        .unwrap();
    let saved = crate::store::agent::providers::get(&database.connection, provider.id)
        .await
        .unwrap()
        .unwrap();
    let snapshot = resolve(
        &database.connection,
        &media::Settings {
            revision: 0,
            bindings: [(Kind::Vision, Some(binding))].into_iter().collect(),
        },
    )
    .await
    .unwrap();
    let resolved = &snapshot[&Kind::Vision].as_ref().unwrap().provider;
    assert!(resolved.models[0].vision);
    assert!(resolved.models[0].tools);
    assert_eq!(
        resolved.oauth,
        sailry_protocol::conversation::oauth::Options::defaults(Authentication::ChatGpt)
    );
    let current = crate::store::agent::providers::get(&database.connection, provider.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current, saved);
}

#[tokio::test]
async fn resolves_catalog_without_rewriting_provider() {
    let (mut provider, _) = fixture();
    provider.credential = None;
    let directory = tempfile::tempdir().unwrap();
    let database = crate::store::database::Database::open(
        &crate::store::tests::profile(&directory).join("node.turso"),
        NodeId([20; 32]),
        None,
    )
    .await
    .unwrap();
    let saved =
        crate::store::agent::providers::put(&database.connection, database.node, &provider, 0)
            .await
            .unwrap();
    let initial = catalog(&database.connection).await.unwrap();
    assert!(initial[0].kinds.contains(&Kind::Vision));
    let reference =
        serde_json::json!({"id":"image-model", "modalities":{"input":["text"],"output":["text"]}});
    database
        .connection
        .execute(
            "INSERT INTO model_catalog(provider,id,body) VALUES(?1,?2,?3)",
            params!["fixture", "image-model", encode(&reference).unwrap()],
        )
        .await
        .unwrap();
    let status = sailry_protocol::conversation::catalog::Status {
        revision: 1,
        models: 1,
        providers: 1,
        ..Default::default()
    };
    database
        .connection
        .execute(
            "INSERT INTO model_catalog_status(id,body) VALUES(1,?1)",
            [encode(&status).unwrap()],
        )
        .await
        .unwrap();
    let resolved = catalog(&database.connection).await.unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].kinds, [Kind::Image, Kind::Video]);
    assert_eq!(
        crate::store::agent::providers::get(&database.connection, provider.id)
            .await
            .unwrap(),
        Some(saved)
    );
}

#[tokio::test]
async fn preserves_settings_revisions() {
    let directory = tempfile::tempdir().unwrap();
    let database = crate::store::database::Database::open(
        &crate::store::tests::profile(&directory).join("node.turso"),
        NodeId([19; 32]),
        None,
    )
    .await
    .unwrap();
    let settings = media::Settings {
        revision: 0,
        bindings: [(Kind::Image, None)].into(),
    };
    let (Output::MediaSettings(saved), event) =
        save(&database.connection, &settings).await.unwrap()
    else {
        panic!("media settings expected")
    };
    assert_eq!(saved.revision, 1);
    assert_eq!(saved.bindings, settings.bindings);
    assert_eq!(event, Some(Event::MediaSettingsChanged(saved.clone())));
    assert_eq!(read(&database.connection).await.unwrap(), saved);
    assert_eq!(
        save(&database.connection, &settings)
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(read(&database.connection).await.unwrap(), saved);
}
