use super::*;
use crate::providers::login::Grant;
use reqwest::header::{HeaderName, HeaderValue};
use sailry_protocol::{Credential, CredentialId, Secret, conversation::Provider};

fn headers(values: &[(&str, &str)]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in values {
        headers.append(
            HeaderName::from_bytes(name.as_bytes()).unwrap(),
            HeaderValue::from_str(value).unwrap(),
        );
    }
    headers
}

#[test]
fn matches_duration_not_position() {
    let (five, seven) = parse(&headers(&[
        ("x-codex-primary-window-minutes", "10080"),
        ("x-codex-primary-used-percent", "12.34"),
        ("x-codex-primary-reset-at", "1700000000"),
        ("x-codex-secondary-window-minutes", "300"),
        ("x-codex-secondary-used-percent", "0"),
    ]));
    assert_eq!(
        five.unwrap(),
        Window {
            used_basis_points: 0,
            reset_at_ms: None
        }
    );
    assert_eq!(
        seven.unwrap(),
        Window {
            used_basis_points: 1234,
            reset_at_ms: Some(1_700_000_000_000),
        }
    );
}

#[test]
fn preserves_unknown_windows() {
    for values in [
        vec![],
        vec![("x-codex-primary-used-percent", "12")],
        vec![("x-codex-primary-window-minutes", "300")],
        vec![
            ("x-codex-primary-window-minutes", "301"),
            ("x-codex-primary-used-percent", "12"),
        ],
        vec![("x-ratelimit-remaining-tokens", "1000")],
    ] {
        assert_eq!(parse(&headers(&values)), (None, None));
    }
}

#[test]
fn rejects_invalid_percentages() {
    for value in ["-1", "NaN", "inf", "invalid", "42949672.96", "1e308"] {
        assert_eq!(
            parse(&headers(&[
                ("x-codex-primary-window-minutes", "300"),
                ("x-codex-primary-used-percent", value),
            ])),
            (None, None),
            "{value}"
        );
    }
}

#[test]
fn preserves_representable_overage() {
    let (five, _) = parse(&headers(&[
        ("x-codex-primary-window-minutes", "300"),
        ("x-codex-primary-used-percent", "100.5"),
    ]));
    let five = five.unwrap();
    assert_eq!(five.used_basis_points, 10_050);
    assert_eq!(five.remaining_basis_points(), 0);
}

#[test]
fn keeps_resets_optional() {
    for value in ["invalid", "-1", "0", "18446744073709551615"] {
        let (five, _) = parse(&headers(&[
            ("x-codex-primary-window-minutes", "300"),
            ("x-codex-primary-used-percent", "1.25"),
            ("x-codex-primary-reset-at", value),
        ]));
        let five = five.unwrap();
        assert_eq!(five.reset_at_ms, None);
        assert_eq!(five.used_basis_points, 125);
    }
    let (five, _) = parse(&headers(&[
        ("x-codex-primary-window-minutes", "300"),
        ("x-codex-primary-used-percent", "2"),
        ("x-codex-primary-reset-at", "1"),
    ]));
    assert!(!five.unwrap().available_at(1000));
}

#[test]
fn rejects_ambiguous_headers() {
    for values in [
        vec![
            ("x-codex-primary-window-minutes", "300"),
            ("x-codex-primary-used-percent", "10"),
            ("x-codex-primary-used-percent", "20"),
        ],
        vec![
            ("x-codex-primary-window-minutes", "300"),
            ("x-codex-primary-used-percent", "10"),
            ("x-codex-secondary-window-minutes", "300"),
            ("x-codex-secondary-used-percent", "20"),
        ],
    ] {
        assert_eq!(parse(&headers(&values)), (None, None));
    }
}

#[test]
fn drops_when_queue_is_full() {
    let (sender, mut receiver) = mpsc::channel(1);
    let observer = observer(
        sender,
        ProviderId::new(),
        CredentialRef {
            node: NodeId([41; 32]),
            id: CredentialId::new(),
        },
        1,
    );
    observer(&HeaderMap::new());
    observer(&HeaderMap::new());
    assert!(matches!(
        receiver.try_recv().unwrap(),
        Job::ProviderQuota(_)
    ));
    assert!(receiver.try_recv().is_err());
    drop(receiver);
    observer(&HeaderMap::new());
}

struct Fixture {
    _directory: tempfile::TempDir,
    database: Database,
    events: broadcast::Sender<EventEnvelope>,
    provider: Provider,
    credential: Credential,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .canonicalize()
            .unwrap()
            .join("node.sqlite3");
        let database = Database::open(&path, NodeId([41; 32]), None).unwrap();
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
        .unwrap();
        let provider = Provider {
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
        configuration::publish(&database.connection, &provider).unwrap();
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

    fn observe(&mut self, observed_ms: u64, used: Option<u32>) {
        let observation = self.observation(observed_ms, used);
        observe(&mut self.database, observation, &self.events).unwrap();
    }

    fn quotas(&self) -> Vec<Snapshot> {
        list(&self.database.connection, self.database.node).unwrap()
    }
}

#[test]
fn accepts_current_binding() {
    let mut fixture = Fixture::new();
    let mut events = fixture.events.subscribe();
    fixture.observe(100, Some(2_500));
    let quota = fixture.quotas().pop().unwrap();
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

#[test]
fn ignores_stale_bindings() {
    for case in ["node", "provider", "credential", "revision"] {
        let mut fixture = Fixture::new();
        let mut observation = fixture.observation(100, Some(2_500));
        match case {
            "node" => observation.credential.node = NodeId([42; 32]),
            "provider" => observation.provider = ProviderId::new(),
            "credential" => observation.credential.id = CredentialId::new(),
            "revision" => observation.revision += 1,
            _ => unreachable!(),
        }
        observe(&mut fixture.database, observation, &fixture.events).unwrap();
        assert!(fixture.quotas().is_empty(), "{case}");
    }
}

#[test]
fn unknown_masks_older_observations() {
    let mut fixture = Fixture::new();
    let mut events = fixture.events.subscribe();
    fixture.observe(100, Some(2_500));
    events.try_recv().unwrap();
    fixture.observe(200, None);
    assert!(matches!(
        events.try_recv().unwrap().event,
        Event::ProviderQuotaChanged { quota: None, .. }
    ));
    fixture.observe(150, Some(5_000));
    assert!(fixture.quotas().is_empty());
    assert!(events.try_recv().is_err());
    fixture.observe(201, None);
    assert!(events.try_recv().is_err());
    fixture.observe(202, Some(10_050));
    assert_eq!(
        fixture.quotas()[0]
            .five_hour
            .as_ref()
            .unwrap()
            .used_basis_points,
        10_050
    );
}

#[test]
fn retains_newer_observations() {
    let mut fixture = Fixture::new();
    fixture.observe(200, Some(5_000));
    fixture.observe(100, Some(2_500));
    let quota = fixture.quotas().pop().unwrap();
    assert_eq!(quota.observed_ms, 200);
    assert_eq!(quota.five_hour.unwrap().used_basis_points, 5_000);
}

#[test]
fn unknown_wins_equal_timestamps() {
    for observations in [[Some(2_500), None], [None, Some(2_500)]] {
        let mut fixture = Fixture::new();
        for used in observations {
            fixture.observe(100, used);
        }
        assert!(fixture.quotas().is_empty());
        assert_eq!(
            configuration::load(&fixture.database.connection, fixture.provider.id)
                .unwrap()
                .unwrap()
                .quota
                .unwrap()
                .observed_ms,
            100
        );
    }
}

#[test]
fn equal_time_keeps_known_values() {
    let mut fixture = Fixture::new();
    let mut events = fixture.events.subscribe();
    fixture.observe(100, Some(2_500));
    events.try_recv().unwrap();
    fixture.observe(100, Some(5_000));
    assert_eq!(
        fixture.quotas()[0]
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
    observe(&mut fixture.database, observation, &fixture.events).unwrap();
    assert!(fixture.quotas()[0].seven_day.is_none());
}

#[test]
fn clears_credential_replacement() {
    let mut fixture = Fixture::new();
    fixture.observe(100, Some(2_500));
    authentication::put(
        &fixture.database.connection,
        fixture.credential.clone(),
        &Fixture::grant(),
    )
    .unwrap();
    let changes = reconcile(&fixture.database.connection, fixture.database.node).unwrap();
    assert_eq!(
        changes,
        vec![Event::ProviderQuotaChanged {
            provider: fixture.provider.id,
            quota: None
        }]
    );
    assert!(fixture.quotas().is_empty());
    fixture.observe(200, Some(5_000));
    assert!(fixture.quotas().is_empty());
}

#[test]
fn clears_changed_provider_binding() {
    for case in ["selection", "disabled", "type", "unlisted"] {
        let mut fixture = Fixture::new();
        fixture.observe(100, Some(2_500));
        let mut stored = configuration::load(&fixture.database.connection, fixture.provider.id)
            .unwrap()
            .unwrap();
        match case {
            "selection" => stored.provider.as_mut().unwrap().credential = None,
            "disabled" => stored.provider.as_mut().unwrap().enabled = false,
            "type" => stored.provider.as_mut().unwrap().api = ModelApi::ChatCompletions,
            "unlisted" => stored.listed = false,
            _ => unreachable!(),
        }
        configuration::save(&fixture.database.connection, &stored).unwrap();
        assert_eq!(
            reconcile(&fixture.database.connection, fixture.database.node)
                .unwrap()
                .len(),
            1,
            "{case}"
        );
        fixture.observe(200, Some(5_000));
        assert!(fixture.quotas().is_empty(), "{case}");
    }
}

#[test]
fn retains_unrelated_provider_edits() {
    let mut fixture = Fixture::new();
    fixture.observe(100, Some(2_500));
    fixture.provider.name = "Renamed fixture".into();
    fixture.provider.revision += 1;
    configuration::publish(&fixture.database.connection, &fixture.provider).unwrap();
    assert!(
        reconcile(&fixture.database.connection, fixture.database.node)
            .unwrap()
            .is_empty()
    );
    assert_eq!(fixture.quotas()[0].observed_ms, 100);
}
