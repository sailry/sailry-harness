use super::*;
use sailry_protocol::Secret;
use serde_json::json;

fn fixture() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../../schema.sql")).unwrap();
    db
}

#[test]
fn retains_previous_values_and_ignores_absent_owners() {
    let connection = fixture();
    let db = &connection;
    let original = json!({"revision":7,"resume":false,"child_concurrency":u64::MAX});
    db.execute(
        "INSERT INTO background_settings(singleton,body) VALUES(1,?1)",
        [encode(&original).unwrap()],
    )
    .unwrap();
    assert_eq!(child_limit(db).unwrap(), None);
    assert_eq!(
        crate::store::background_settings::retained(db).unwrap(),
        Some(u64::MAX)
    );
    let mut info = super::fixture::install(db, "neutral-policy");
    assert_eq!(child_limit(db).unwrap(), Some(u64::MAX));
    let frozen = settings::read(db, &info.summary.reference()).unwrap();
    assert_eq!(frozen.values["parallel"], json!(u64::MAX.to_string()));
    super::fixture::configure(db, 3);
    info = required(db, "neutral-policy").unwrap();
    migrate(db, &mut info).unwrap();
    assert_eq!(child_limit(db).unwrap(), Some(3));
    assert_eq!(
        settings::read_admitted(
            db,
            &Info {
                summary: plugin::Summary {
                    settings_revision: frozen.package.settings_revision,
                    ..info.summary.clone()
                },
                ..info.clone()
            }
        )
        .unwrap()
        .values,
        frozen.values
    );
    assert_eq!(
        crate::store::background_settings::retained(db).unwrap(),
        Some(u64::MAX)
    );
    info.summary.enabled = false;
    super::super::save(db, &info, true).unwrap();
    assert_eq!(child_limit(db).unwrap(), Some(3));
    super::super::save(db, &info, false).unwrap();
    assert_eq!(child_limit(db).unwrap(), None);
}

#[test]
fn prevents_competing_policy_owners() {
    let connection = fixture();
    let db = &connection;
    let mut info = super::fixture::install(db, "neutral-policy");
    info.summary.name = "another-policy".into();
    assert_eq!(
        check(db, &info).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    info.summary.name = "neutral-policy".into();
    check(db, &info).unwrap();
    info.extension.as_mut().unwrap().scope = plugin::Scope::Desktop;
    assert_eq!(
        check(db, &info).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn preserves_unrelated_secret_bindings_during_migration() {
    let connection = fixture();
    let db = &connection;
    let mut info = super::fixture::install(db, "neutral-policy");
    let original: Field = serde_json::from_value(
        json!({"type":"string","x-sailry-secret":{"server":"original","header":"authorization"}}),
    )
    .unwrap();
    let mut saved = settings::stored(db, &info).unwrap();
    saved.values.clear();
    let previous = saved.schema.as_mut().unwrap();
    previous.properties.clear();
    previous.required = vec![];
    previous.properties.insert("token".into(), original.clone());
    saved.slots.insert(
        "token".into(),
        Some(Secret::new("synthetic-private-value".into())),
    );
    settings::persist(db, &mut info, &saved).unwrap();
    let mut replacement = original.clone();
    replacement.secret.as_mut().unwrap().server = Some("replacement".into());
    info.settings
        .as_mut()
        .unwrap()
        .properties
        .insert("token".into(), replacement);
    super::super::save(db, &info, true).unwrap();
    db.execute(
        "INSERT INTO background_settings(singleton,body) VALUES(1,?1)",
        [encode(&json!({"revision":1,"resume":true,"child_concurrency":13})).unwrap()],
    )
    .unwrap();
    migrate(db, &mut info).unwrap();
    let migrated = settings::stored(db, &info).unwrap();
    assert_eq!(migrated.schema.unwrap().properties["token"], original);
    assert_eq!(migrated.slots, saved.slots);
    assert_eq!(migrated.values["parallel"], json!("13"));
}
