use super::*;
use sailry_protocol::{
    Secret,
    plugin::oauth::{Configuration, Provider},
};
use serde_json::json;
use std::collections::BTreeMap;

fn fixture() -> (tempfile::TempDir, Connection, Info, Grant) {
    let directory = tempfile::tempdir().unwrap();
    let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
    let mut info = host.install_bundled("files").unwrap();
    info.settings = Some(serde_json::from_value(json!({
        "$schema":plugin::settings::SCHEMA,"type":"object","additionalProperties":false,
        "properties":{"token":{"type":"string","x-sailry-secret":{"origin":"https://www.googleapis.com","header":"Authorization","prefix":"Bearer "}}}
    })).unwrap());
    let declaration = Declaration {
        provider: Provider::Google,
        scopes: vec!["https://www.googleapis.com/auth/drive.readonly".into()],
        credentials: vec!["token".into()],
    };
    info.extension.as_mut().unwrap().oauth = Some(declaration.clone());
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../../schema.sql")).unwrap();
    super::super::save(&db, &info, true).unwrap();
    let grant = Grant {
        declaration,
        configuration: Configuration {
            client_id: "client".into(),
            client_secret: None,
            tenant: None,
        },
        access_token: Secret::new("original-token".into()),
        refresh_token: Some(Secret::new("refresh-token".into())),
        expires_at_ms: 1,
    };
    (directory, db, info, grant)
}

#[test]
fn renews_captures_and_rejects_stale_writers() {
    let (_directory, db, info, grant) = fixture();
    let info = save(&db, &info.summary.reference(), grant.clone()).unwrap();
    let first = info.summary.reference();
    let id = settings::stored(&db, &info).unwrap().oauth.unwrap().id;
    let saved = settings::save(
        &db,
        &first,
        &BTreeMap::new(),
        &BTreeMap::from([("token".into(), plugin::settings::SecretUpdate::Keep)]),
    )
    .unwrap();
    let mut renewed = grant;
    renewed.access_token = Secret::new("renewed-token".into());
    renew(&db, &first.name, id, 1, renewed.clone()).unwrap();
    assert_eq!(load(&db, &first.name, id).unwrap().0, 2);
    assert!(renew(&db, &first.name, id, 1, renewed).is_err());
    for reference in [first, saved.package.clone()] {
        let mut captured = info.clone();
        captured.summary.settings_revision = reference.settings_revision;
        let resolved = settings::resolve(&db, &captured).unwrap();
        assert_eq!(resolved.secrets["token"].expose(), "renewed-token");
    }
    let revoked = revoke(&db, &saved.package).unwrap();
    assert!(!read(&db, &revoked.summary.reference()).unwrap().configured);
    assert!(load(&db, &info.summary.name, id).is_err());
    for (_, stored) in settings::revisions(&db, &info.summary.name).unwrap() {
        assert!(stored.slots.get("token").is_none_or(Option::is_none));
        assert!(
            stored
                .oauth
                .as_ref()
                .is_none_or(|authorization| authorization.grant.is_none())
        );
    }
}

#[test]
fn secret_replacement_invalidates_captured_account() {
    let (_directory, db, info, grant) = fixture();
    let info = save(&db, &info.summary.reference(), grant).unwrap();
    let id = settings::stored(&db, &info).unwrap().oauth.unwrap().id;
    let saved = settings::save(
        &db,
        &info.summary.reference(),
        &BTreeMap::new(),
        &BTreeMap::from([(
            "token".into(),
            plugin::settings::SecretUpdate::Replace(Secret::new("manual-token".into())),
        )]),
    )
    .unwrap();
    assert!(load(&db, &info.summary.name, id).is_err());
    assert!(!read(&db, &saved.package).unwrap().configured);
    assert_eq!(
        settings::read_secret(&db, &saved.package, "token")
            .unwrap()
            .unwrap()
            .expose(),
        "manual-token"
    );
}
