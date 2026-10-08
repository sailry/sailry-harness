use super::*;
use sailry_protocol::conversation::ModelApi;
use sailry_protocol::media::Generation;

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
            vision: true,
            tools: true,
            reasoning: true,
            web_search: false,
            generates: vec![Generation::Image],
            efforts: vec![Effort::Low, Effort::High],
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
fn follows_admitted_configuration() {
    let (provider, config) = fixture();
    let models = for_turn(Snapshot::new(), Some(&provider), &config);
    assert_eq!(models.len(), 2);
    for kind in [Kind::Vision, Kind::Image] {
        let model = &models[&kind];
        assert_eq!(model.provider.id, config.provider);
        assert_eq!(model.model, config.model);
        assert_eq!(model.provider.credential, config.credential);
        assert_eq!(model.effort, Some(config.effort));
    }
    let mut changed = config.clone();
    changed.model = "missing-model".into();
    assert!(for_turn(Snapshot::new(), Some(&provider), &changed).is_empty());
    assert!(for_turn(Snapshot::new(), None, &config).is_empty());
}

#[test]
fn preserves_disabled_and_explicit_choices() {
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
    assert_eq!(models.len(), 1);
    assert_eq!(
        models[&Kind::Vision].provider.credential,
        provider.credential
    );
    assert_ne!(models[&Kind::Vision].provider.credential, config.credential);
    assert_eq!(models[&Kind::Vision].effort, None);
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
    let binding = media::Binding {
        provider: provider.id,
        model: "image-model".into(),
    };
    crate::store::agent::providers::put(&database.connection, database.node, &provider, 0).unwrap();
    let snapshot = resolve(
        &database.connection,
        &media::Settings {
            revision: 0,
            bindings: [(Kind::Vision, Some(binding))].into_iter().collect(),
        },
    )
    .unwrap();
    let provider = &snapshot[&Kind::Vision].as_ref().unwrap().provider;
    assert_eq!(
        provider.oauth,
        sailry_protocol::conversation::oauth::Options::defaults(Authentication::ChatGpt)
    );
}

#[test]
fn keeps_runtime_effort_out_of_snapshots() {
    let (provider, config) = fixture();
    let model = for_turn(Snapshot::new(), Some(&provider), &config)
        .remove(&Kind::Image)
        .unwrap();
    let body = serde_json::to_value(&model).unwrap();
    assert!(body.get("effort").is_none());
    let restored: Model = serde_json::from_value(body).unwrap();
    assert_eq!(restored.effort, None);
    assert_eq!(restored.provider.credential, config.credential);
}
