use super::*;

fn account() -> Record {
    Record {
        id: RequestId::new(),
        plugin: "fixture".into(),
        connector: "account".into(),
        revision: 3,
        auth_revision: 2,
        method: Kind::Secret,
        enabled: true,
        fields: BTreeMap::from([("label".into(), "original".into())]),
        secrets: BTreeMap::from([("token".into(), Secret::new("private-original".into()))]),
        grant: Some(Grant {
            access: Secret::new("private-access".into()),
            refresh: Some(Secret::new("private-refresh".into())),
            expires_at_ms: Some(4_102_444_800_000),
            extra: BTreeMap::new(),
        }),
        grant_revision: 5,
        identity: Some(Identity {
            id: "account".into(),
            name: "Account".into(),
        }),
        bound: None,
        state: State::Connected,
        checked_at_ms: Some(10),
        login: None,
    }
}

fn pending(changed: bool) -> (Record, Record, RequestId) {
    let previous = account();
    let mut record = previous.clone();
    record.revision += 1;
    record.state = State::Connecting;
    if changed {
        record.auth_revision = record.revision;
        record
            .secrets
            .insert("token".into(), Secret::new("private-pending".into()));
        record.grant = None;
    }
    let attempt = RequestId::new();
    begin(&mut record, attempt, Some(previous.clone()));
    validate(&record).unwrap();
    (record, previous, attempt)
}

#[test]
fn preserves_storage_contracts() {
    for (scenario, check) in [
        (
            "preserves_renewals_during_the_pending_attempt",
            (|scenario: &'static str| {
                let (mut record, previous, attempt) = pending(false);
                assert!(usable(&record), "{scenario}");
                record.grant_revision += 1;
                record.grant.as_mut().unwrap().access = Secret::new("private-renewed".into());
                record.fields.insert("label".into(), "current".into());
                let revision = record.grant_revision;
                assert!(restore(&mut record, attempt), "{scenario}");
                assert_eq!(record.state, previous.state, "{scenario}");
                assert_eq!(
                    record.grant.as_ref().unwrap().access.expose(),
                    "private-renewed",
                    "{scenario}"
                );
                assert_eq!(record.grant_revision, revision, "{scenario}");
                assert_eq!(record.fields["label"], "original", "{scenario}");
                assert!(record.login.is_none(), "{scenario}");

                let (mut record, previous, attempt) = pending(false);
                record.grant_revision += 1;
                record.grant = None;
                record.state = State::Expired;
                record.fields.insert("label".into(), "current".into());
                assert!(
                    finish(&mut record, attempt, Outcome::Failed).unwrap(),
                    "{scenario}"
                );
                assert_eq!(record.state, State::Expired, "{scenario}");
                assert!(record.grant.is_none(), "{scenario}");
                assert_eq!(record.fields, previous.fields, "{scenario}");
                assert_eq!(record.secrets, previous.secrets, "{scenario}");

                let (mut changed, _, _) = pending(true);
                assert!(!usable(&changed), "{scenario}");
                changed.state = State::Expired;
                let attempt = changed.login.as_ref().unwrap().attempt;
                assert!(restore(&mut changed, attempt), "{scenario}");
                assert_eq!(changed.state, State::Expired, "{scenario}");
                assert!(changed.grant.is_none(), "{scenario}");
            }) as fn(&'static str),
        ),
        (
            "restores_only_the_owned_attempt",
            (|scenario: &'static str| {
                for changed in [false, true] {
                    let (mut record, previous, attempt) = pending(changed);
                    let before = encode(&record).unwrap();
                    assert!(!restore(&mut record, RequestId::new()), "{scenario}");
                    assert_eq!(encode(&record).unwrap(), before, "{scenario}");
                    let revisions = (record.revision, record.auth_revision, record.grant_revision);
                    assert!(restore(&mut record, attempt), "{scenario}");
                    assert!(record.login.is_none(), "{scenario}");
                    assert_eq!(record.state, State::Connected, "{scenario}");
                    assert_eq!(record.secrets, previous.secrets, "{scenario}");
                    assert_eq!(
                        record.grant.as_ref().map(|grant| &grant.access),
                        previous.grant.as_ref().map(|grant| &grant.access),
                        "{scenario}"
                    );
                    assert_eq!(
                        (record.revision, record.auth_revision, record.grant_revision),
                        revisions,
                        "{scenario}"
                    );
                    validate(&record).unwrap();
                }
            }),
        ),
        (
            "preserves_newer_edits_without_reviving_credentials",
            (|scenario: &'static str| {
                for changed in [false, true] {
                    let (mut record, _, attempt) = pending(changed);
                    record.revision += 1;
                    record.fields.insert("label".into(), "newer edit".into());
                    let secrets = record.secrets.clone();
                    let revision = record.revision;
                    assert!(restore(&mut record, attempt), "{scenario}");
                    assert_eq!(record.fields["label"], "newer edit", "{scenario}");
                    assert_eq!(record.secrets, secrets, "{scenario}");
                    assert_eq!(record.revision, revision, "{scenario}");
                    assert_eq!(
                        record.state,
                        if changed {
                            State::NotConnected
                        } else {
                            State::Connected
                        },
                        "{scenario}"
                    );
                    assert_eq!(record.grant.is_none(), changed, "{scenario}");
                    assert!(record.login.is_none(), "{scenario}");
                }
            }),
        ),
        (
            "leaves_revocation_fences_and_values_authoritative",
            (|scenario: &'static str| {
                for grant in [false, true] {
                    let (mut record, _, attempt) = pending(false);
                    record.revision += 1;
                    if grant {
                        record.grant_revision += 1;
                    } else {
                        record.auth_revision = record.revision;
                    }
                    record.grant = None;
                    record
                        .secrets
                        .insert("token".into(), Secret::new("private-revoked".into()));
                    let revisions = (record.revision, record.auth_revision, record.grant_revision);
                    assert!(restore(&mut record, attempt), "{scenario}");
                    assert_eq!(record.state, State::NotConnected, "{scenario}");
                    assert!(record.grant.is_none(), "{scenario}");
                    assert_eq!(
                        record.secrets["token"].expose(),
                        "private-revoked",
                        "{scenario}"
                    );
                    assert_eq!(
                        (record.revision, record.auth_revision, record.grant_revision),
                        revisions,
                        "{scenario}"
                    );
                }
            }),
        ),
        (
            "rejects_foreign_or_nested_baselines",
            (|scenario: &'static str| {
                for foreign in [false, true] {
                    let (mut record, _, _) = pending(false);
                    let nested = record.login.clone();
                    let previous = record.login.as_mut().unwrap().previous.as_mut().unwrap();
                    if foreign {
                        previous.id = RequestId::new();
                    } else {
                        previous.login = nested;
                    }
                    assert!(validate(&record).is_err(), "{scenario}");
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
