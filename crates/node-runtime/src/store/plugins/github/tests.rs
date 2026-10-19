use super::*;

fn identity() -> Identity {
    Identity {
        login: "fixture-user".into(),
        source: "keyring".into(),
    }
}

fn fixture() -> (tempfile::TempDir, crate::plugins::Host, Connection, Info) {
    let directory = tempfile::tempdir().unwrap();
    let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
    let info = host.install_bundled("github").unwrap();
    std::fs::create_dir_all(directory.path().join("storage")).unwrap();
    let db = Connection::open(
        directory
            .path()
            .canonicalize()
            .unwrap()
            .join("storage/node.sqlite3"),
    )
    .unwrap();
    db.execute_batch(include_str!("../../schema.sql")).unwrap();
    super::super::save(&db, &info, true).unwrap();
    (directory, host, db, info)
}

#[test]
fn confines_changed_bindings() {
    for change in ["server", "header", "prefix", "env", "origin", "unsupported"] {
        let (_directory, host, db, mut info) = fixture();
        let binding = info
            .settings
            .as_mut()
            .unwrap()
            .properties
            .get_mut("token")
            .unwrap()
            .secret
            .as_mut()
            .unwrap();
        match change {
            "server" => binding.server = Some("other".into()),
            "header" => binding.header = Some("X-Token".into()),
            "prefix" => binding.prefix = Some("Other ".into()),
            "env" => binding.env = Some("TOKEN".into()),
            "origin" => binding.origin = Some("https://changed.invalid".into()),
            "unsupported" => binding.unsupported = true,
            _ => unreachable!(),
        }
        super::super::save(&db, &info, true).unwrap();
        assert_eq!(
            prepare(&db, &host, &info.summary.reference())
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let rows: i64 = db
            .query_row("SELECT count(*) FROM plugin_settings", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 0);
    }
}

#[test]
fn confines_saved_endpoint_changes() {
    let (_directory, host, db, mut info) = fixture();
    let mut stored = settings::stored(&db, &info).unwrap();
    let mut configuration = host
        .mcp_configuration(
            &info.summary.reference(),
            sailry_link::CancellationToken::new(),
        )
        .unwrap();
    let plugin::mcp::Server::Http { url, .. } = configuration.servers.get_mut("github").unwrap()
    else {
        unreachable!()
    };
    *url = "https://changed.invalid/mcp/".into();
    stored.mcp = Some(configuration);
    settings::persist(&db, &mut info, &stored).unwrap();
    assert_eq!(
        save(&db, &host, &info.summary.reference(), identity())
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert!(
        settings::read(&db, &info.summary.reference())
            .unwrap()
            .configured
            .is_empty()
    );
}

#[test]
fn binds_the_imported_slot_without_public_plaintext() {
    let (_directory, host, db, info) = fixture();
    let before = info.summary.reference();
    let transaction = db.unchecked_transaction().unwrap();
    let (output, event) = save(&transaction, &host, &before, identity()).unwrap();
    transaction.commit().unwrap();
    let Output::PluginSettings(state) = &output else {
        panic!("settings expected")
    };
    assert_eq!(state.configured, ["token"]);
    assert_eq!(
        state.package.settings_revision,
        before.settings_revision + 1
    );
    assert!(
        !serde_json::to_string(&(&output, &event))
            .unwrap()
            .contains("synthetic-token")
    );
    let current = required(&db, "github").unwrap();
    let resolved = settings::resolve(&db, &current).unwrap();
    assert!(resolved.secrets.is_empty());
    let captured = resolved.github.unwrap();
    assert_eq!(captured.login, "fixture-user");
    let saved = settings::stored(&db, &current).unwrap();
    assert!(!saved.slots.contains_key("token"));
    assert!(saved.github.is_some());
    assert!(
        settings::read_secret(&db, &state.package, "token")
            .unwrap()
            .is_none()
    );
}

#[test]
fn keeps_links_and_preserves_explicit_manual_tokens() {
    let (_directory, host, db, info) = fixture();
    let (output, _) = save(&db, &host, &info.summary.reference(), identity()).unwrap();
    let Output::PluginSettings(state) = output else {
        panic!("settings expected")
    };
    let kept = settings::save(
        &db,
        &state.package,
        &state.values,
        &std::collections::BTreeMap::from([("token".into(), plugin::settings::SecretUpdate::Keep)]),
    )
    .unwrap();
    assert!(
        settings::stored(&db, &required(&db, "github").unwrap())
            .unwrap()
            .github
            .is_some()
    );
    let manual = settings::save(
        &db,
        &kept.package,
        &kept.values,
        &std::collections::BTreeMap::from([(
            "token".into(),
            plugin::settings::SecretUpdate::Replace(sailry_protocol::Secret::new(
                "manual-token".into(),
            )),
        )]),
    )
    .unwrap();
    let current = required(&db, "github").unwrap();
    assert!(settings::stored(&db, &current).unwrap().github.is_none());
    assert_eq!(
        settings::read_secret(&db, &manual.package, "token")
            .unwrap()
            .unwrap()
            .expose(),
        "manual-token"
    );
    let resolved = settings::resolve(&db, &current).unwrap();
    assert!(resolved.github.is_none());
    assert_eq!(resolved.secrets["token"].expose(), "manual-token");
}

#[test]
fn requires_declaration_without_name_dispatch() {
    let (_directory, host, db, mut info) = fixture();
    info.summary.name = "repository-tools".into();
    // Keep an explicit configuration so the captured package need not be acquired again.
    let configuration = host
        .mcp_configuration(
            &required(&db, "github").unwrap().summary.reference(),
            sailry_link::CancellationToken::new(),
        )
        .unwrap();
    super::super::save(&db, &info, true).unwrap();
    let mut stored = settings::stored(&db, &info).unwrap();
    stored.mcp = Some(configuration);
    settings::persist(&db, &mut info, &stored).unwrap();
    assert!(prepare(&db, &host, &info.summary.reference()).is_ok());
    info.extension.as_mut().unwrap().credential_source = None;
    super::super::save(&db, &info, true).unwrap();
    assert!(prepare(&db, &host, &info.summary.reference()).is_err());
}
