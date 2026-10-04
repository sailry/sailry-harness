use super::*;
use sailry_protocol::Secret;
use serde_json::json;

#[test]
fn unsupported_bindings_never_reuse_credentials() {
    for required in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
        let mut info = host.install_bundled("files").unwrap();
        let schema: Schema = serde_json::from_value(json!({
            "$schema": SCHEMA, "type":"object", "additionalProperties":false,
            "properties":{"token":{"type":"string","x-sailry-secret":{
                "origin":"https://example.com", "header":"Authorization", "future_target":true
            }}},
            "required": if required { vec!["token"] } else { vec![] }
        }))
        .unwrap();
        info.settings = Some(schema.clone());
        info.summary.settings_revision = 1;
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../../schema.sql")).unwrap();
        super::super::save(&db, &info, true).unwrap();
        let stored = Stored {
            schema: Some(schema),
            values: BTreeMap::new(),
            slots: BTreeMap::from([("token".into(), Some(Secret::new("fixture-secret".into())))]),
            authorizations: BTreeMap::new(),
            mcp: None,
        };
        let body = encode(&stored).unwrap();
        db.execute(
            "INSERT INTO plugin_settings(plugin,revision,body) VALUES(?1,1,?2)",
            params![info.summary.name, &body],
        )
        .unwrap();

        let package = info.summary.reference();
        let state = read(&db, &package).unwrap();
        assert!(!state.ready);
        assert!(state.configured.is_empty());
        assert!(state.keepable.is_empty());
        assert!(read_secret(&db, &package, "token").is_err());
        assert_eq!(
            resolve(&db, &info).err().unwrap().code,
            ErrorCode::NotConfigured
        );
        for update in [
            SecretUpdate::Keep,
            SecretUpdate::Replace(Secret::new("replacement".into())),
            SecretUpdate::Clear,
        ] {
            let error = save(
                &db,
                &package,
                &BTreeMap::new(),
                &BTreeMap::from([("token".into(), update)]),
            )
            .unwrap_err();
            assert_eq!(error.code, ErrorCode::Unavailable);
        }
        let unchanged: Vec<u8> = db
            .query_row("SELECT body FROM plugin_settings", [], |row| row.get(0))
            .unwrap();
        assert_eq!(unchanged, body);
        let saved: Stored = serde_json::from_slice(&unchanged).unwrap();
        assert_eq!(
            saved.slots["token"].as_ref().unwrap().expose(),
            "fixture-secret"
        );
        assert_eq!(
            super::super::required(&db, &info.summary.name).unwrap(),
            info
        );
    }
}

#[test]
fn omitted_saved_values_remain_empty() {
    let stored: Stored = serde_json::from_value(json!({})).unwrap();
    assert!(stored.schema.is_none());
    assert!(stored.values.is_empty());
    assert!(stored.slots.is_empty());
    assert!(stored.authorizations.is_empty());
}
