use super::*;
use crate::providers::login::Grant;
use sailry_protocol::{Credential, CredentialId, Secret, conversation::Provider};
struct Fixture {
    _directory: tempfile::TempDir,
    database: Database,
    events: broadcast::Sender<EventEnvelope>,
    provider: Provider,
    credential: Credential,
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().canonicalize().unwrap().join("node.turso");
        let database = Database::open(&path, NodeId([41; 32]), None).await.unwrap();
        let provider_id = ProviderId::new();
        let credential = authentication::put(
            &database.connection,
            Credential {
                id: CredentialId::new(),
                provider: provider_id,
                authentication: Authentication::ChatGpt,
                account_label: None,
                revision: 0,
                expires_at_ms: None,
                revoked: false,
            },
            &Self::grant(),
        )
        .await
        .unwrap();
        let provider = Provider {
            request_max_retries: None,
            stream_max_retries: None,
            preset: None,
            binding: None,
            consent: None,
            id: provider_id,
            revision: 1,
            name: "Quota fixture".into(),
            api: ModelApi::Responses,
            authentication: Authentication::ChatGpt,
            endpoint: crate::providers::login::CHATGPT_ENDPOINT.into(),
            options: None,
            oauth: None,
            enabled: true,
            models: Vec::new(),
            default_model: String::new(),
            credential: Some(CredentialRef {
                node: database.node,
                id: credential.id,
            }),
        };
        configuration::publish(&database.connection, &provider)
            .await
            .unwrap();
        let (events, _) = broadcast::channel(64);
        Self {
            _directory: directory,
            database,
            events,
            provider,
            credential,
        }
    }

    fn grant() -> Secret {
        Grant::ChatGpt {
            access: Secret::new("isolated-quota-access".into()),
            refresh: Secret::new("isolated-quota-refresh".into()),
            expires_at_ms: 4_000_000_000_000,
            earliest_refresh_at: None,
            account_id: None,
            client_id: "quota-fixture".into(),
            subject: "quota-subject".into(),
            email: None,
            id_token: Secret::new("isolated-quota-identity".into()),
            scopes: vec!["resource.invoke".into(), "chatgpt.tokens.use.direct".into()],
            renewal_pending: false,
        }
        .encode()
        .unwrap()
    }

    fn observation(&self, observed_ms: u64, used: Option<u32>) -> Observation {
        Observation {
            provider: self.provider.id,
            credential: self.provider.credential.clone().unwrap(),
            revision: self.credential.revision,
            observed_ms,
            five_hour: used.map(|used_basis_points| Window {
                used_basis_points,
                reset_at_ms: None,
            }),
            seven_day: None,
        }
    }

    async fn observe(&mut self, observed_ms: u64, used: Option<u32>) {
        let observation = self.observation(observed_ms, used);
        observe(&mut self.database, observation, &self.events)
            .await
            .unwrap();
    }

    async fn quotas(&self) -> Vec<Snapshot> {
        list(&self.database.connection, self.database.node)
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn accepts_current_binding() {
    let mut fixture = Fixture::new().await;
    let mut events = fixture.events.subscribe();
    fixture.observe(100, Some(2_500)).await;
    let quota = fixture.quotas().await.pop().unwrap();
    assert_eq!(
        quota.credential,
        fixture.provider.credential.clone().unwrap()
    );
    assert_eq!(quota.revision, fixture.credential.revision);
    assert_eq!(quota.observed_ms, 100);
    assert_eq!(
        events.try_recv().unwrap().event,
        Event::ProviderQuotaChanged {
            provider: fixture.provider.id,
            quota: Some(quota),
        }
    );
}

#[tokio::test]
async fn ignores_stale_bindings() {
    for case in ["node", "provider", "credential", "revision"] {
        let mut fixture = Fixture::new().await;
        let mut observation = fixture.observation(100, Some(2_500));
        match case {
            "node" => observation.credential.node = NodeId([42; 32]),
            "provider" => observation.provider = ProviderId::new(),
            "credential" => observation.credential.id = CredentialId::new(),
            "revision" => observation.revision += 1,
            _ => unreachable!(),
        }
        observe(&mut fixture.database, observation, &fixture.events)
            .await
            .unwrap();
        assert!(fixture.quotas().await.is_empty(), "{case}");
    }
}

#[tokio::test]
async fn unknown_masks_older_observations() {
    let mut fixture = Fixture::new().await;
    let mut events = fixture.events.subscribe();
    fixture.observe(100, Some(2_500)).await;
    events.try_recv().unwrap();
    fixture.observe(200, None).await;
    assert!(matches!(
        events.try_recv().unwrap().event,
        Event::ProviderQuotaChanged { quota: None, .. }
    ));
    fixture.observe(150, Some(5_000)).await;
    assert!(fixture.quotas().await.is_empty());
    assert!(events.try_recv().is_err());
    fixture.observe(201, None).await;
    assert!(events.try_recv().is_err());
    fixture.observe(202, Some(10_050)).await;
    assert_eq!(
        fixture.quotas().await[0]
            .five_hour
            .as_ref()
            .unwrap()
            .used_basis_points,
        10_050
    );
}

#[tokio::test]
async fn retains_newer_observations() {
    let mut fixture = Fixture::new().await;
    fixture.observe(200, Some(5_000)).await;
    fixture.observe(100, Some(2_500)).await;
    let quota = fixture.quotas().await.pop().unwrap();
    assert_eq!(quota.observed_ms, 200);
    assert_eq!(quota.five_hour.unwrap().used_basis_points, 5_000);
}

#[tokio::test]
async fn unknown_wins_equal_timestamps() {
    for observations in [[Some(2_500), None], [None, Some(2_500)]] {
        let mut fixture = Fixture::new().await;
        for used in observations {
            fixture.observe(100, used).await;
        }
        assert!(fixture.quotas().await.is_empty());
        assert_eq!(
            configuration::load(&fixture.database.connection, fixture.provider.id)
                .await
                .unwrap()
                .unwrap()
                .quota
                .unwrap()
                .observed_ms,
            100
        );
    }
}

#[tokio::test]
async fn equal_time_keeps_known_values() {
    let mut fixture = Fixture::new().await;
    let mut events = fixture.events.subscribe();
    fixture.observe(100, Some(2_500)).await;
    events.try_recv().unwrap();
    fixture.observe(100, Some(5_000)).await;
    assert_eq!(
        fixture.quotas().await[0]
            .five_hour
            .as_ref()
            .unwrap()
            .used_basis_points,
        2_500
    );
    assert!(events.try_recv().is_err());
    let mut observation = fixture.observation(100, Some(5_000));
    observation.seven_day = Some(Window {
        used_basis_points: 100,
        reset_at_ms: None,
    });
    observe(&mut fixture.database, observation, &fixture.events)
        .await
        .unwrap();
    assert!(fixture.quotas().await[0].seven_day.is_none());
}

#[tokio::test]
async fn clears_credential_replacement() {
    let mut fixture = Fixture::new().await;
    fixture.observe(100, Some(2_500)).await;
    authentication::put(
        &fixture.database.connection,
        fixture.credential.clone(),
        &Fixture::grant(),
    )
    .await
    .unwrap();
    let changes = reconcile(&fixture.database.connection, fixture.database.node)
        .await
        .unwrap();
    assert_eq!(
        changes,
        vec![Event::ProviderQuotaChanged {
            provider: fixture.provider.id,
            quota: None
        }]
    );
    assert!(fixture.quotas().await.is_empty());
    fixture.observe(200, Some(5_000)).await;
    assert!(fixture.quotas().await.is_empty());
}

#[tokio::test]
async fn clears_changed_provider_binding() {
    for case in ["selection", "disabled", "type", "unlisted"] {
        let mut fixture = Fixture::new().await;
        fixture.observe(100, Some(2_500)).await;
        let mut stored = configuration::load(&fixture.database.connection, fixture.provider.id)
            .await
            .unwrap()
            .unwrap();
        match case {
            "selection" => stored.provider.as_mut().unwrap().credential = None,
            "disabled" => stored.provider.as_mut().unwrap().enabled = false,
            "type" => stored.provider.as_mut().unwrap().api = ModelApi::ChatCompletions,
            "unlisted" => stored.listed = false,
            _ => unreachable!(),
        }
        configuration::save(&fixture.database.connection, &stored)
            .await
            .unwrap();
        assert_eq!(
            reconcile(&fixture.database.connection, fixture.database.node)
                .await
                .unwrap()
                .len(),
            1,
            "{case}"
        );
        fixture.observe(200, Some(5_000)).await;
        assert!(fixture.quotas().await.is_empty(), "{case}");
    }
}

#[tokio::test]
async fn retains_unrelated_provider_edits() {
    let mut fixture = Fixture::new().await;
    fixture.observe(100, Some(2_500)).await;
    fixture.provider.name = "Renamed fixture".into();
    fixture.provider.revision += 1;
    configuration::publish(&fixture.database.connection, &fixture.provider)
        .await
        .unwrap();
    assert!(
        reconcile(&fixture.database.connection, fixture.database.node)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(fixture.quotas().await[0].observed_ms, 100);
}
