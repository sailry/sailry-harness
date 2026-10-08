use super::*;
use sailry_protocol::conversation::ModelApi;

fn fixture() -> (Provider, SessionConfig) {
    let provider = Provider {
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
            web_search: false,
            generates: vec![],
            efforts: vec![Effort::Low, Effort::High],
            capabilities: None,
            custom_efforts: false,
            default_effort: Effort::Low,
        }],
    };
    let config = SessionConfig {
        assistant: None,
        resource: None,
        provider: provider.id,
        model: "image-model".into(),
        effort: Effort::High,
        mode: WorkMode::Code,
        permission: Permission::Ask,
        credential: Some(CredentialRef {
            node: NodeId([17; 32]),
            id: CredentialId::new(),
        }),
    };
    (provider, config)
}

#[test]
fn plans_admitted_configuration() {
    let (provider, config) = fixture();
    let models = for_turn(Snapshot::new(), Some(&provider), &config);
    assert_eq!(models.len(), 3);
    for kind in [Kind::Vision, Kind::Image, Kind::Video] {
        let plan = &models[&kind];
        let model = plan.primary.as_ref().unwrap();
        assert_eq!(model.provider.id, config.provider);
        assert_eq!(model.model, config.model);
        assert_eq!(model.provider.credential, config.credential);
        assert_eq!(model.effort, Some(config.effort));
        assert!(plan.fallback.is_none());
    }
    let mut changed = config.clone();
    changed.model = "missing-model".into();
    let models = for_turn(Snapshot::new(), Some(&provider), &changed);
    assert_eq!(models.len(), 3);
    assert!(models.values().all(|plan| {
        plan.primary.as_ref().unwrap().model == "missing-model" && plan.fallback.is_none()
    }));
    assert!(for_turn(Snapshot::new(), None, &config).is_empty());
}

#[test]
fn preserves_disabled_and_frozen_fallback() {
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
    assert_eq!(models.len(), 2);
    assert!(!models.contains_key(&Kind::Image));
    let vision = &models[&Kind::Vision];
    assert_eq!(
        vision.primary.as_ref().unwrap().provider.credential,
        config.credential
    );
    let fallback = vision.fallback.as_ref().unwrap();
    assert_eq!(fallback.provider.credential, provider.credential);
    assert_ne!(fallback.provider.credential, config.credential);
    assert_eq!(fallback.effort, None);
    assert!(models[&Kind::Video].primary.is_some());
    assert!(models[&Kind::Video].fallback.is_none());
}

#[test]
fn keeps_fallback_without_primary() {
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
    assert_eq!(models.len(), 1);
    let plan = &models[&Kind::Vision];
    assert!(plan.primary.is_none());
    assert_eq!(
        plan.fallback.as_ref().unwrap().provider.credential,
        provider.credential
    );
    assert!(!models.contains_key(&Kind::Video));
}

#[test]
fn captures_login_override_options() {
    let (mut provider, _) = fixture();
    let directory = tempfile::tempdir().unwrap();
    let database = crate::store::database::Database::open(
        &directory
            .path()
            .canonicalize()
            .unwrap()
            .join("node.sqlite3"),
        NodeId([18; 32]),
        None,
    )
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
    crate::store::agent::providers::put(&database.connection, database.node, &provider, 0).unwrap();
    let saved = crate::store::agent::providers::get(&database.connection, provider.id)
        .unwrap()
        .unwrap();
    let snapshot = resolve(
        &database.connection,
        &media::Settings {
            revision: 0,
            bindings: [(Kind::Vision, Some(binding))].into_iter().collect(),
        },
    )
    .unwrap();
    let resolved = &snapshot[&Kind::Vision].as_ref().unwrap().provider;
    assert!(resolved.models[0].vision);
    assert!(resolved.models[0].tools);
    assert_eq!(
        resolved.oauth,
        sailry_protocol::conversation::oauth::Options::defaults(Authentication::ChatGpt)
    );
    let current = crate::store::agent::providers::get(&database.connection, provider.id)
        .unwrap()
        .unwrap();
    assert_eq!(current, saved);
}

#[test]
fn resolves_catalog_without_rewriting_provider() {
    let (mut provider, _) = fixture();
    provider.credential = None;
    let directory = tempfile::tempdir().unwrap();
    let database = crate::store::database::Database::open(
        &directory
            .path()
            .canonicalize()
            .unwrap()
            .join("node.sqlite3"),
        NodeId([20; 32]),
        None,
    )
    .unwrap();
    let saved =
        crate::store::agent::providers::put(&database.connection, database.node, &provider, 0)
            .unwrap();
    let initial = catalog(&database.connection).unwrap();
    assert!(initial[0].kinds.contains(&Kind::Vision));
    let reference =
        serde_json::json!({"id":"image-model", "modalities":{"input":["text"],"output":["text"]}});
    database
        .connection
        .execute(
            "INSERT INTO model_catalog(provider,id,body) VALUES(?1,?2,?3)",
            params!["fixture", "image-model", encode(&reference).unwrap()],
        )
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
        .unwrap();
    let resolved = catalog(&database.connection).unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].kinds, [Kind::Image, Kind::Video]);
    assert_eq!(
        crate::store::agent::providers::get(&database.connection, provider.id).unwrap(),
        Some(saved)
    );
}

#[test]
fn preserves_settings_revisions() {
    let directory = tempfile::tempdir().unwrap();
    let database = crate::store::database::Database::open(
        &directory
            .path()
            .canonicalize()
            .unwrap()
            .join("node.sqlite3"),
        NodeId([19; 32]),
        None,
    )
    .unwrap();
    let settings = media::Settings {
        revision: 0,
        bindings: [(Kind::Image, None)].into(),
    };
    let (Output::MediaSettings(saved), event) = save(&database.connection, &settings).unwrap()
    else {
        panic!("media settings expected")
    };
    assert_eq!(saved.revision, 1);
    assert_eq!(saved.bindings, settings.bindings);
    assert_eq!(event, Some(Event::MediaSettingsChanged(saved.clone())));
    assert_eq!(read(&database.connection).unwrap(), saved);
    assert_eq!(
        save(&database.connection, &settings).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(read(&database.connection).unwrap(), saved);
}

#[test]
fn keeps_runtime_effort_out_of_snapshots() {
    let (provider, config) = fixture();
    let model = for_turn(Snapshot::new(), Some(&provider), &config)
        .remove(&Kind::Image)
        .unwrap()
        .primary
        .unwrap();
    let body = serde_json::to_value(&model).unwrap();
    assert!(body.get("effort").is_none());
    let restored: Model = serde_json::from_value(body).unwrap();
    assert_eq!(restored.effort, None);
    assert_eq!(restored.provider.credential, config.credential);
    let snapshot: Snapshot = [(Kind::Image, Some(restored)), (Kind::Video, None)].into();
    let body = serde_json::to_value(&snapshot).unwrap();
    assert!(body["image"].get("effort").is_none());
    assert!(body["image"].get("primary").is_none());
    assert!(body["image"].get("fallback").is_none());
    assert_eq!(body["video"], serde_json::Value::Null);
    let restored: Snapshot = serde_json::from_value(body).unwrap();
    assert_eq!(
        restored[&Kind::Image].as_ref().unwrap().provider.credential,
        config.credential
    );
    assert!(restored[&Kind::Video].is_none());
}
