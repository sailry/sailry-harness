use super::*;

fn configuration(server: plugin::mcp::Server) -> plugin::mcp::Configuration {
    plugin::mcp::Configuration {
        schema: plugin::mcp::SCHEMA.into(),
        servers: BTreeMap::from([("service".into(), server)]),
    }
}

fn http(url: &str) -> plugin::mcp::Configuration {
    configuration(plugin::mcp::Server::Http {
        exclusive: false,
        keep_images: None,
        reuse: true,
        url: url.into(),
        headers: BTreeMap::new(),
    })
}

async fn fixture() -> (tempfile::TempDir, Connection, Info, Stored) {
    let directory = tempfile::tempdir().unwrap();
    let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
    let mut info = host.install_bundled("files").unwrap();
    let schema:Schema=serde_json::from_value(json!({
        "$schema":SCHEMA,"type":"object","additionalProperties":false,
        "properties":{"token":{"type":"string","x-sailry-secret":{"server":"service","header":"Authorization"}}},
        "required":["token"]
    })).unwrap();
    info.settings = Some(schema.clone());
    info.summary.settings_revision = 1;
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(include_str!("../../../schema.sql"))
        .await
        .unwrap();
    super::super::super::save(&db, &info, true).await.unwrap();
    db.execute(
        "UPDATE plugins SET settings_revision=?1 WHERE name=?2",
        params![
            info.summary.settings_revision as i64,
            info.summary.name.as_str()
        ],
    )
    .await
    .unwrap();
    let stored = Stored {
        schema: Some(schema),
        values: BTreeMap::new(),
        slots: BTreeMap::from([("token".into(), Some(Secret::new("preserved-token".into())))]),
        mcp_targets: Default::default(),
        authorizations: Default::default(),
        mcp: Some(http("https://original.test/mcp")),
        mcp_allowed: BTreeMap::new(),
        github: None,
        oauth: None,
    };
    db.execute(
        "INSERT INTO plugin_settings(plugin,revision,body) VALUES(?1,1,?2)",
        params![info.summary.name.as_str(), encode(&stored).unwrap()],
    )
    .await
    .unwrap();
    (directory, db, info, stored)
}

async fn replace(db: &Connection, info: &Info, stored: &Stored) {
    db.execute(
        "UPDATE plugin_settings SET body=?1 WHERE plugin=?2",
        params![encode(stored).unwrap(), info.summary.name.as_str()],
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn fences_slots_retired_into_connections() {
    let (directory, db, info, mut saved) = fixture().await;
    saved.mcp_targets = super::super::targets::current(&db, &info, saved.mcp.as_ref())
        .await
        .unwrap();
    replace(&db, &info, &saved).await;
    let original = encode(&saved).unwrap();
    assert_eq!(
        resolve(&db, &info).await.unwrap().secrets["token"].expose(),
        "preserved-token"
    );
    let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
    let connector = host
        .install_bundled("figma")
        .unwrap()
        .extension
        .unwrap()
        .connectors;
    let mut current = info.clone();
    current.extension.as_mut().unwrap().connectors = connector;
    super::super::super::save(&db, &current, true)
        .await
        .unwrap();
    // An unrelated connector does not revoke an ordinary, unchanged slot.
    assert_eq!(
        resolve(&db, &info).await.unwrap().secrets["token"].expose(),
        "preserved-token"
    );
    // Collection may remove an unused latest revision from an inventory tombstone.
    current.summary.settings_revision = 2;
    super::super::super::save(&db, &current, false)
        .await
        .unwrap();
    assert_eq!(
        resolve(&db, &info).await.unwrap().secrets["token"].expose(),
        "preserved-token"
    );
    current.summary.settings_revision = 1;
    for installed in [true, false] {
        current.settings = None;
        super::super::super::save(&db, &current, installed)
            .await
            .unwrap();
        assert_eq!(
            resolve(&db, &info).await.err().unwrap().code,
            ErrorCode::NotConfigured
        );
        current.settings = info.settings.clone();
        current
            .settings
            .as_mut()
            .unwrap()
            .properties
            .get_mut("token")
            .unwrap()
            .secret
            .as_mut()
            .unwrap()
            .header = Some("X-Other".into());
        super::super::super::save(&db, &current, installed)
            .await
            .unwrap();
        assert_eq!(
            resolve(&db, &info).await.err().unwrap().code,
            ErrorCode::NotConfigured
        );
        let retained: Vec<u8> = (async {
            let mut statement = db
                .prepare("SELECT body FROM plugin_settings WHERE plugin=?1 AND revision=1")
                .await?;
            let row = statement.query_row([info.summary.name.as_str()]).await?;
            let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
            mapped
        })
        .await
        .unwrap();
        assert_eq!(retained, original);
    }
}

#[tokio::test]
async fn preserves_unbound_bytes_without_injecting_them() {
    let (_directory, db, info, saved) = fixture().await;
    let package = info.summary.reference();
    let state = read(&db, &package).await.unwrap();
    assert!(!state.ready);
    assert!(state.keepable.is_empty());
    assert!(state.configured.is_empty());
    assert!(read_secret(&db, &package, "token").await.is_err());
    assert!(resolve(&db, &info).await.is_err());
    assert!(
        save(
            &db,
            &package,
            &BTreeMap::new(),
            &BTreeMap::from([("token".into(), SecretUpdate::Keep)])
        )
        .await
        .is_err()
    );
    let unchanged = stored(&db, &info).await.unwrap();
    assert_eq!(
        unchanged.slots["token"].as_ref().unwrap().expose(),
        saved.slots["token"].as_ref().unwrap().expose()
    );
    let transaction = db.unchecked_transaction().await.unwrap();
    let rebound = save(
        &transaction,
        &package,
        &BTreeMap::new(),
        &BTreeMap::from([(
            "token".into(),
            SecretUpdate::Replace(Secret::new("replacement-token".into())),
        )]),
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    assert!(rebound.ready);
    let package = rebound.package;
    assert_eq!(
        read_secret(&db, &package, "token")
            .await
            .unwrap()
            .unwrap()
            .expose(),
        "replacement-token"
    );
    let transaction = db.unchecked_transaction().await.unwrap();
    let cleared = save(
        &transaction,
        &package,
        &BTreeMap::new(),
        &BTreeMap::from([("token".into(), SecretUpdate::Clear)]),
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    assert!(cleared.configured.is_empty());
}

#[tokio::test]
async fn requires_rebinding_after_real_server_changes() {
    let (_directory, db, info, mut saved) = fixture().await;
    saved.mcp_targets = super::super::targets::current(&db, &info, saved.mcp.as_ref())
        .await
        .unwrap();
    replace(&db, &info, &saved).await;
    let state = read(&db, &info.summary.reference()).await.unwrap();
    assert!(state.ready);
    assert_eq!(state.keepable, vec!["token".to_owned()]);
    for configuration in [
        http("https://other.test/mcp"),
        configuration(plugin::mcp::Server::Sse {
            exclusive: false,
            keep_images: None,
            reuse: true,
            url: "https://original.test/mcp".into(),
            headers: Default::default(),
        }),
        configuration(plugin::mcp::Server::Stdio {
            exclusive: false,
            keep_images: None,
            reuse: true,
            command: "other".into(),
            args: vec!["serve".into()],
            env: Default::default(),
            cwd: "${PLUGIN_ROOT}".into(),
        }),
    ] {
        let mut changed: Stored = serde_json::from_slice(&encode(&saved).unwrap()).unwrap();
        changed.mcp = Some(configuration);
        replace(&db, &info, &changed).await;
        assert!(!read(&db, &info.summary.reference()).await.unwrap().ready);
        assert!(
            read_secret(&db, &info.summary.reference(), "token")
                .await
                .is_err()
        );
        assert!(
            save(
                &db,
                &info.summary.reference(),
                &BTreeMap::new(),
                &BTreeMap::from([("token".into(), SecretUpdate::Keep)])
            )
            .await
            .is_err()
        );
        assert_eq!(
            stored(&db, &info).await.unwrap().slots["token"]
                .as_ref()
                .unwrap()
                .expose(),
            "preserved-token"
        );
    }
    replace(&db, &info, &saved).await;
    let mut updated = info.clone();
    updated.summary.digest = "a".repeat(64);
    super::super::super::save(&db, &updated, true)
        .await
        .unwrap();
    let transaction = db.unchecked_transaction().await.unwrap();
    let kept = save(
        &transaction,
        &updated.summary.reference(),
        &BTreeMap::new(),
        &BTreeMap::from([("token".into(), SecretUpdate::Keep)]),
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    assert!(kept.ready);
    assert_eq!(
        read_secret(&db, &kept.package, "token")
            .await
            .unwrap()
            .unwrap()
            .expose(),
        "preserved-token"
    );
}

#[tokio::test]
async fn hashes_stdio_command_arguments_environment_and_directory() {
    let (_directory, db, info, _saved) = fixture().await;
    let original = configuration(plugin::mcp::Server::Stdio {
        exclusive: false,
        keep_images: None,
        reuse: true,
        command: "server".into(),
        args: vec!["--stdio".into()],
        env: BTreeMap::from([("MODE".into(), "read".into())]),
        cwd: "${PLUGIN_ROOT}".into(),
    });
    let expected = super::super::targets::current(&db, &info, Some(&original))
        .await
        .unwrap();
    for field in ["command", "args", "env", "cwd"] {
        let mut changed = original.clone();
        let plugin::mcp::Server::Stdio {
            command,
            args,
            env,
            cwd,
            ..
        } = changed.servers.get_mut("service").unwrap()
        else {
            unreachable!()
        };
        match field {
            "command" => *command = "other".into(),
            "args" => args.push("--other".into()),
            "env" => {
                env.insert("MODE".into(), "write".into());
            }
            "cwd" => *cwd = "${WORKTREE_ROOT}".into(),
            _ => unreachable!(),
        }
        assert_ne!(
            super::super::targets::current(&db, &info, Some(&changed))
                .await
                .unwrap(),
            expected,
            "{field}"
        );
    }
    assert_eq!(
        super::super::targets::current(&db, &info, Some(&original))
            .await
            .unwrap(),
        expected
    );
}

#[tokio::test]
async fn keeps_empty_slots_without_binding_credentials() {
    for cleared in [false, true] {
        let (_directory, db, info, mut saved) = fixture().await;
        saved.slots.clear();
        if cleared {
            saved.slots.insert("token".into(), None);
        }
        replace(&db, &info, &saved).await;
        let state = read(&db, &info.summary.reference()).await.unwrap();
        assert!(state.configured.is_empty());
        assert_eq!(state.keepable, ["token"]);
        let transaction = db.unchecked_transaction().await.unwrap();
        let state = save(
            &transaction,
            &info.summary.reference(),
            &BTreeMap::new(),
            &BTreeMap::from([("token".into(), SecretUpdate::Keep)]),
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        assert!(!state.ready);
        assert!(state.configured.is_empty());
        let current = super::super::super::required(&db, &info.summary.name)
            .await
            .unwrap();
        let stored = stored(&db, &current).await.unwrap();
        assert!(!stored.slots.get("token").is_some_and(Option::is_some));
        assert_eq!(
            stored.mcp_targets,
            super::super::targets::current(&db, &current, stored.mcp.as_ref())
                .await
                .unwrap()
        );
    }
}

#[tokio::test]
async fn preserves_private_history_without_active_bindings() {
    let (_directory, db, info, mut saved) = fixture().await;
    saved.github = Some(super::super::history::Identity {
        login: "fixture-user".into(),
        source: "keyring".into(),
    });
    saved.authorizations.insert(
        "service".into(),
        super::super::history::McpGrant {
            id: sailry_protocol::CredentialId::new(),
            endpoint: "https://original.test/mcp".into(),
            revision: 1,
            credentials: Some(Secret::new("private-history".into())),
        },
    );
    replace(&db, &info, &saved).await;
    let roundtrip = stored(&db, &info).await.unwrap();
    assert_eq!(
        roundtrip.authorizations["service"]
            .credentials
            .as_ref()
            .unwrap()
            .expose(),
        "private-history"
    );
    assert_eq!(roundtrip.github.as_ref().unwrap().login, "fixture-user");
    assert!(
        !serde_json::to_string(&read(&db, &info.summary.reference()).await.unwrap())
            .unwrap()
            .contains("private-history")
    );
}
