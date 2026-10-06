use super::*;
use sailry_protocol::{
    Authentication, CredentialId, CredentialRef, ProviderId,
    conversation::{ModelApi, Provider, quota},
};

fn provider() -> Provider {
    Provider {
        id: ProviderId::new(),
        revision: 1,
        name: "Quota fixture".into(),
        api: ModelApi::Responses,
        authentication: Authentication::ChatGpt,
        endpoint: "https://api.openai.com/v1".into(),
        options: None,
        oauth: None,
        enabled: true,
        models: vec![],
        default_model: "fixture".into(),
        credential: Some(CredentialRef {
            node: NodeId([1; 32]),
            id: CredentialId::new(),
        }),
    }
}

fn observation(provider: &Provider) -> quota::Snapshot {
    quota::Snapshot {
        provider: provider.id,
        credential: provider.credential.clone().unwrap(),
        revision: 1,
        observed_ms: 100,
        five_hour: Some(quota::Window {
            used_basis_points: 4250,
            reset_at_ms: Some(200),
        }),
        seven_day: None,
    }
}

fn change(cursor: u64, event: Event) -> Update {
    Update::Event(EventEnvelope {
        node: NodeId([1; 32]),
        cursor,
        event,
    })
}

#[test]
fn orders_updates_and_clears_unknown() {
    let provider = provider();
    let mut state = snapshot(0);
    state.providers.push(provider.clone());
    let mut projection = Projection::new(state.node, 1);
    projection.apply(1, Update::Snapshot(state)).unwrap();
    let quota = observation(&provider);
    let update = change(
        1,
        Event::ProviderQuotaChanged {
            provider: provider.id,
            quota: Some(quota.clone()),
        },
    );
    assert_eq!(projection.apply(1, update.clone()).unwrap(), Apply::Applied);
    assert_eq!(projection.apply(1, update).unwrap(), Apply::Ignored);
    assert_eq!(
        projection.snapshot().unwrap().provider_quotas,
        vec![quota.clone()]
    );

    let mut newer = quota;
    newer.observed_ms = 120;
    newer.five_hour.as_mut().unwrap().used_basis_points = 10050;
    projection
        .apply(
            1,
            change(
                2,
                Event::ProviderQuotaChanged {
                    provider: provider.id,
                    quota: Some(newer.clone()),
                },
            ),
        )
        .unwrap();
    assert_eq!(projection.snapshot().unwrap().provider_quotas, vec![newer]);
    projection
        .apply(
            1,
            change(
                3,
                Event::ProviderQuotaChanged {
                    provider: provider.id,
                    quota: None,
                },
            ),
        )
        .unwrap();
    assert!(projection.snapshot().unwrap().provider_quotas.is_empty());
    assert_eq!(projection.snapshot().unwrap().cursor, 3);
}

#[test]
fn rejects_unselected_and_foreign_bindings() {
    let provider = provider();
    let valid = observation(&provider);
    let mut foreign = valid.clone();
    foreign.credential.node = NodeId([2; 32]);
    let mut unselected = valid.clone();
    unselected.credential.id = CredentialId::new();
    let mut absent = valid.clone();
    absent.provider = ProviderId::new();
    let mut state = snapshot(0);
    state.providers.push(provider.clone());
    state.provider_quotas = vec![
        foreign.clone(),
        unselected.clone(),
        absent.clone(),
        valid.clone(),
    ];
    let mut projection = Projection::new(state.node, 1);
    projection.apply(1, Update::Snapshot(state)).unwrap();
    assert_eq!(projection.snapshot().unwrap().provider_quotas, vec![valid]);

    for (index, quota) in [foreign, unselected, absent].into_iter().enumerate() {
        assert_eq!(
            projection
                .apply(
                    1,
                    change(
                        index as u64 + 1,
                        Event::ProviderQuotaChanged {
                            provider: provider.id,
                            quota: Some(quota),
                        }
                    )
                )
                .unwrap(),
            Apply::Applied
        );
        assert!(projection.snapshot().unwrap().provider_quotas.is_empty());
    }
}

#[test]
fn drops_replaced_and_removed_credentials() {
    let mut provider = provider();
    let mut state = snapshot(0);
    state.providers.push(provider.clone());
    state.provider_quotas.push(observation(&provider));
    let mut projection = Projection::new(state.node, 1);
    projection.apply(1, Update::Snapshot(state)).unwrap();
    provider.credential.as_mut().unwrap().id = CredentialId::new();
    provider.revision += 1;
    projection
        .apply(1, change(1, Event::ProviderChanged(provider.clone())))
        .unwrap();
    assert!(projection.snapshot().unwrap().provider_quotas.is_empty());
    let quota = observation(&provider);
    projection
        .apply(
            1,
            change(
                2,
                Event::ProviderQuotaChanged {
                    provider: provider.id,
                    quota: Some(quota.clone()),
                },
            ),
        )
        .unwrap();
    assert_eq!(projection.snapshot().unwrap().provider_quotas, vec![quota]);
    projection
        .apply(1, change(3, Event::ProviderRemoved { id: provider.id }))
        .unwrap();
    assert!(projection.snapshot().unwrap().provider_quotas.is_empty());
    assert!(projection.snapshot().unwrap().providers.is_empty());
}

#[test]
fn reconnects_from_authoritative_snapshot() {
    let provider = provider();
    let mut state = snapshot(0);
    state.providers.push(provider.clone());
    state.provider_quotas.push(observation(&provider));
    let mut projection = Projection::new(state.node, 1);
    projection
        .apply(1, Update::Snapshot(state.clone()))
        .unwrap();
    let clear = change(
        2,
        Event::ProviderQuotaChanged {
            provider: provider.id,
            quota: None,
        },
    );
    assert_eq!(projection.apply(1, clear.clone()).unwrap(), Apply::Recover);
    assert_eq!(
        projection.snapshot().unwrap().provider_quotas,
        state.provider_quotas
    );
    projection.reconnect(2).unwrap();
    assert_eq!(projection.apply(1, clear.clone()).unwrap(), Apply::Ignored);
    state.cursor = 2;
    state.provider_quotas.clear();
    projection
        .apply(2, Update::Snapshot(state.clone()))
        .unwrap();
    assert_eq!(projection.apply(2, clear).unwrap(), Apply::Ignored);
    assert_eq!(projection.snapshot(), Some(&state));
}

#[test]
fn preserves_observed_values_without_presentation() {
    let provider = provider();
    let mut quota = observation(&provider);
    quota.five_hour = Some(quota::Window {
        used_basis_points: 10050,
        reset_at_ms: Some(1),
    });
    let mut state = snapshot(0);
    state.providers.push(provider);
    state.provider_quotas.push(quota.clone());
    let mut projection = Projection::new(state.node, 1);
    projection.apply(1, Update::Snapshot(state)).unwrap();
    assert_eq!(projection.snapshot().unwrap().provider_quotas, vec![quota]);
}
