use super::*;
use serde_json::json;

fn target() -> Target {
    Target {
        id: Id::new(),
        revision: 2,
        grant_revision: 3,
        package: Reference {
            name: "fixture".into(),
            digest: "fixture".into(),
            settings_revision: 0,
        },
        declaration: serde_json::from_value(json!({
            "id":"fixture","display":{"label":"Fixture"},"accounts":"single",
            "origins":["https://service.example.com"],
            "auth":[{"kind":"secret","fields":["alpha","beta"]}],
            "fields":[{"name":"alpha","title":"Alpha","secret":true},
                {"name":"beta","title":"Beta","secret":true}],
            "inject":[{"origin":"https://service.example.com","header":"Authorization"}]
        }))
        .unwrap(),
        method: Kind::Secret,
        fields: BTreeMap::new(),
        secrets: BTreeMap::from([
            ("alpha".into(), Secret::new("alpha-private-token".into())),
            ("beta".into(), Secret::new("beta-private-token".into())),
        ]),
        identity: None,
        grant: None,
        mcp_endpoint: None,
        manual: false,
        checking: false,
    }
}

#[test]
fn selects_explicit_fields() {
    let target = target();
    let mut injection = target.declaration.inject[0].clone();
    let default = target.credential(&injection).unwrap();
    assert!(default == Credential::Field("alpha".into()));
    injection.field = Some("beta".into());
    let selected = target.credential(&injection).unwrap();
    assert!(selected == Credential::Field("beta".into()));
    assert!(selected != default);
    assert_eq!(
        selected.read(&target).unwrap(),
        target.secrets.get("beta").cloned()
    );
}

#[test]
fn distinguishes_equal_field_values() {
    let mut target = target();
    let value = target.secrets["alpha"].clone();
    target.secrets.insert("beta".into(), value);
    let mut injection = target.declaration.inject[0].clone();
    let first = target.credential(&injection).unwrap();
    injection.field = Some("beta".into());
    let sealed = target.credential(&injection).unwrap();
    assert!(first != sealed);
    assert_eq!(first.read(&target).unwrap(), sealed.read(&target).unwrap());
    target.secrets.remove("beta");
    assert_eq!(
        sealed.read(&target).unwrap_err().code,
        ErrorCode::NotConfigured
    );
    assert!(first.read(&target).unwrap().is_some());
}

#[test]
fn keeps_grants_distinct() {
    let mut target = target();
    target.method = Kind::Oauth2;
    target.declaration.auth = vec![serde_json::from_value(json!({
        "kind":"oauth2","flow":"pkce","authorize":"https://service.example.com/authorize",
        "token":"https://service.example.com/token","client":{"bundled":{"client_id":"fixture"}},
        "scopes":[],"refresh":true
    }))
    .unwrap()];
    target.grant = Some(Issued {
        access: Secret::new("grant-private-token".into()),
        refresh: None,
        expires_at_ms: None,
        account_label: None,
        extra: BTreeMap::new(),
    });
    let mut injection = target.declaration.inject[0].clone();
    let grant = target.credential(&injection).unwrap();
    assert!(grant == Credential::Grant);
    assert_eq!(
        grant.read(&target).unwrap(),
        Some(Secret::new("grant-private-token".into()))
    );
    injection.field = Some("beta".into());
    let field = target.credential(&injection).unwrap();
    assert!(field == Credential::Field("beta".into()));
    assert_eq!(
        field.read(&target).unwrap(),
        target.secrets.get("beta").cloned()
    );
    target.grant = None;
    assert_eq!(
        grant.read(&target).unwrap_err().code,
        ErrorCode::NotConfigured
    );
}

#[test]
fn rejects_missing_fields() {
    let mut target = target();
    let field = Credential::Field("beta".into());
    target.secrets.remove("beta");
    assert_eq!(
        field.read(&target).unwrap_err().code,
        ErrorCode::NotConfigured
    );
    target.secrets.insert(
        "undeclared".into(),
        Secret::new("unused-private-token".into()),
    );
    assert_eq!(
        Credential::Field("undeclared".into())
            .read(&target)
            .unwrap_err()
            .code,
        ErrorCode::NotConfigured
    );
}

#[test]
fn omits_optional_fields() {
    let mut target = target();
    target.declaration.fields[1].optional = true;
    target.secrets.remove("beta");
    assert_eq!(
        Credential::Field("beta".into()).read(&target).unwrap(),
        None
    );
    assert_eq!(
        Credential::Field("alpha".into()).read(&target).unwrap(),
        target.secrets.get("alpha").cloned()
    );
}
