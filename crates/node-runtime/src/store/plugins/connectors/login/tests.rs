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
fn preserves_renewals_during_the_pending_attempt() {
    let (mut record, previous, attempt) = pending(false);
    assert!(usable(&record));
    record.grant_revision += 1;
    record.grant.as_mut().unwrap().access = Secret::new("private-renewed".into());
    record.fields.insert("label".into(), "current".into());
    let revision = record.grant_revision;
    assert!(restore(&mut record, attempt));
    assert_eq!(record.state, previous.state);
    assert_eq!(
        record.grant.as_ref().unwrap().access.expose(),
        "private-renewed"
    );
    assert_eq!(record.grant_revision, revision);
    assert_eq!(record.fields["label"], "original");
    assert!(record.login.is_none());

    let (mut record, previous, attempt) = pending(false);
    record.grant_revision += 1;
    record.grant = None;
    record.state = State::Expired;
    record.fields.insert("label".into(), "current".into());
    assert!(finish(&mut record, attempt, Outcome::Failed).unwrap());
    assert_eq!(record.state, State::Expired);
    assert!(record.grant.is_none());
    assert_eq!(record.fields, previous.fields);
    assert_eq!(record.secrets, previous.secrets);

    let (mut changed, _, _) = pending(true);
    assert!(!usable(&changed));
    changed.state = State::Expired;
    let attempt = changed.login.as_ref().unwrap().attempt;
    assert!(restore(&mut changed, attempt));
    assert_eq!(changed.state, State::Expired);
    assert!(changed.grant.is_none());
}

#[test]
fn restores_only_the_owned_attempt() {
    for changed in [false, true] {
        let (mut record, previous, attempt) = pending(changed);
        let before = encode(&record).unwrap();
        assert!(!restore(&mut record, RequestId::new()));
        assert_eq!(encode(&record).unwrap(), before);
        let revisions = (record.revision, record.auth_revision, record.grant_revision);
        assert!(restore(&mut record, attempt));
        assert!(record.login.is_none());
        assert_eq!(record.state, State::Connected);
        assert_eq!(record.secrets, previous.secrets);
        assert_eq!(
            record.grant.as_ref().map(|grant| &grant.access),
            previous.grant.as_ref().map(|grant| &grant.access)
        );
        assert_eq!(
            (record.revision, record.auth_revision, record.grant_revision),
            revisions
        );
        validate(&record).unwrap();
    }
}

#[test]
fn preserves_newer_edits_without_reviving_credentials() {
    for changed in [false, true] {
        let (mut record, _, attempt) = pending(changed);
        record.revision += 1;
        record.fields.insert("label".into(), "newer edit".into());
        let secrets = record.secrets.clone();
        let revision = record.revision;
        assert!(restore(&mut record, attempt));
        assert_eq!(record.fields["label"], "newer edit");
        assert_eq!(record.secrets, secrets);
        assert_eq!(record.revision, revision);
        assert_eq!(
            record.state,
            if changed {
                State::NotConnected
            } else {
                State::Connected
            }
        );
        assert_eq!(record.grant.is_none(), changed);
        assert!(record.login.is_none());
    }
}

#[test]
fn leaves_revocation_fences_and_values_authoritative() {
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
        assert!(restore(&mut record, attempt));
        assert_eq!(record.state, State::NotConnected);
        assert!(record.grant.is_none());
        assert_eq!(record.secrets["token"].expose(), "private-revoked");
        assert_eq!(
            (record.revision, record.auth_revision, record.grant_revision),
            revisions
        );
    }
}

#[test]
fn rejects_foreign_or_nested_baselines() {
    for foreign in [false, true] {
        let (mut record, _, _) = pending(false);
        let nested = record.login.clone();
        let previous = record.login.as_mut().unwrap().previous.as_mut().unwrap();
        if foreign {
            previous.id = RequestId::new();
        } else {
            previous.login = nested;
        }
        assert!(validate(&record).is_err());
    }
}
