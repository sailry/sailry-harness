use super::*;

fn fixture() -> (tempfile::TempDir, crate::plugins::Host, Connection) {
    let directory = tempfile::tempdir().unwrap();
    let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
    let mut db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../../schema.sql")).unwrap();
    db.execute(
        "INSERT INTO node(singleton,identity) VALUES(1,?1)",
        [&[1u8; 32][..]],
    )
    .unwrap();
    install(&mut db, &host).unwrap();
    (directory, host, db)
}

fn changed(db: &Connection, name: &str, version: &str) -> Info {
    let mut info = required(db, name).unwrap();
    info.extension.as_mut().unwrap().settings_page = None;
    info.summary.version = Some(version.into());
    // Simulate a previous bundle and retain its immutable parsed metadata.
    let digest = blake3::hash(&encode(&info).unwrap()).to_hex().to_string();
    assert_ne!(digest, info.summary.digest);
    info.summary.digest = digest;
    let mut package = info.clone();
    package.origin = None;
    db.execute(
        "INSERT INTO plugin_packages(digest,name,body) VALUES(?1,?2,?3)",
        params![package.summary.digest, name, encode(&package).unwrap()],
    )
    .unwrap();
    save(db, &info, true).unwrap();
    info
}

fn captured(db: &Connection, info: &Info) -> Info {
    let (Output::Plugin(package), _) = execute(
        db,
        None,
        &Command::ReadPluginVersion {
            package: info.summary.reference(),
        },
    )
    .unwrap() else {
        panic!("captured plugin expected")
    };
    package
}

#[test]
fn adds_missing_defaults_without_reinstalling_records() {
    let (_directory, host, mut db) = fixture();
    let mut memory = required(&db, "memory").unwrap();
    memory.summary.enabled = false;
    save(&db, &memory, true).unwrap();
    let search = required(&db, "web-search").unwrap();
    save(&db, &search, false).unwrap();
    db.execute("DELETE FROM plugins WHERE name='code'", [])
        .unwrap();
    let mut settings = crate::store::code::settings(&db).unwrap();
    settings.enabled = false;
    settings.after_edit = false;
    let settings = crate::store::code::save(&db, &settings, settings.revision).unwrap();
    let before = list(&db).unwrap();
    let events: i64 = db
        .query_row("SELECT count(*) FROM events", [], |row| row.get(0))
        .unwrap();
    db.execute_batch("CREATE TEMP TRIGGER fail_default BEFORE INSERT ON plugins WHEN NEW.name='code' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    assert!(install(&mut db, &host).is_err());
    assert_eq!(list(&db).unwrap(), before);
    assert_eq!(
        db.query_row("SELECT count(*) FROM events", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        events
    );
    db.execute_batch("DROP TRIGGER fail_default").unwrap();
    install(&mut db, &host).unwrap();
    let code = required(&db, "code").unwrap();
    assert!(!code.summary.enabled);
    assert_eq!(crate::store::code::settings(&db).unwrap(), settings);
    assert_eq!(code.summary.revision, 1);
    assert_eq!(
        code.extension
            .unwrap()
            .settings_page
            .unwrap()
            .entry
            .as_deref(),
        Some("dev.sailry.platform/desktop/settings.js")
    );
    assert_eq!(required(&db, "memory").unwrap(), memory);
    assert!(get(&db, "web-search").unwrap().is_none());
    let mut after = list(&db).unwrap();
    after.retain(|summary| summary.name != "code");
    assert_eq!(after, before);
    let recorded: Vec<u8> = db
        .query_row(
            "SELECT body FROM plugins WHERE name='web-search'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(recorded, encode(&search).unwrap());
    let installed = list(&db).unwrap();
    install(&mut db, &host).unwrap();
    assert_eq!(list(&db).unwrap(), installed);
    assert_eq!(
        db.query_row("SELECT count(*) FROM events", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        events + 1
    );
}

#[test]
fn refresh_preserves_configuration_and_private_records() {
    let (_directory, host, mut db) = fixture();
    let mut before = changed(&db, "code", "0.0.1");
    before.summary.enabled = false;
    before.summary.settings_revision = 7;
    save(&db, &before, true).unwrap();
    let settings = serde_json::json!({
        "schema":null,"values":{"draft":"retained", "model":"fixture/model"},
        "slots":{},"authorizations":{}
    });
    let body = serde_json::to_vec(&settings).unwrap();
    db.execute(
        "INSERT INTO plugin_settings(plugin,revision,body) VALUES('code',7,?1)",
        [&body],
    )
    .unwrap();
    db.execute(
        "INSERT INTO plugin_values(name,key,revision,value) VALUES('code','draft',1,?1)",
        [&b"{\"text\":\"retained\"}"[..]],
    )
    .unwrap();
    let retained = captured(&db, &before);
    install(&mut db, &host).unwrap();
    let after = required(&db, "code").unwrap();
    assert_eq!(after.summary.revision, before.summary.revision + 1);
    assert_eq!(after.summary.settings_revision, 7);
    assert!(!after.summary.enabled);
    assert_eq!(after.origin, Some(plugin::Origin::Bundled));
    assert_eq!(
        after.summary.version,
        crate::plugins::Host::bundled_version("code").unwrap()
    );
    assert_eq!(
        after.summary.digest,
        crate::plugins::Host::bundled_digest("code").unwrap()
    );
    assert_ne!(after.summary.digest, before.summary.digest);
    assert_eq!(captured(&db, &before), retained);
    assert_eq!(
        after
            .extension
            .unwrap()
            .settings_page
            .unwrap()
            .entry
            .as_deref(),
        Some("dev.sailry.platform/desktop/settings.js")
    );
    let saved: Vec<u8> = db
        .query_row(
            "SELECT body FROM plugin_settings WHERE plugin='code' AND revision=7",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(saved, body);
    let value: Vec<u8> = db
        .query_row(
            "SELECT value FROM plugin_values WHERE name='code' AND key='draft'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(value, b"{\"text\":\"retained\"}");
}

#[test]
fn skips_custom_unchanged_and_removed_packages() {
    let (_directory, host, mut db) = fixture();
    for name in [
        "code",
        "web-search",
        "browser",
        "files",
        "office",
        "commands",
        "memory",
        "brief",
    ] {
        let original = required(&db, name).unwrap();
        let previous = changed(&db, name, "0.0.1");
        for origin in [
            None,
            Some(plugin::Origin::Directory),
            Some(plugin::Origin::Archive),
            Some(plugin::Origin::Worktree {
                worktree: sailry_protocol::WorktreeId::new(),
                path: "custom-package".into(),
            }),
            Some(plugin::Origin::Online {
                source: plugin::skills::Resolved {
                    repository: "https://github.com/example/custom.git".into(),
                    git_ref: "main".into(),
                    commit: "a".repeat(40),
                },
                path: name.into(),
            }),
        ] {
            let mut custom = previous.clone();
            custom.summary.version = Some("0.0.1".into());
            custom.origin = origin;
            save(&db, &custom, true).unwrap();
            install(&mut db, &host).unwrap();
            assert_eq!(required(&db, name).unwrap(), custom);
        }
        let mut newer = semver::Version::parse(original.summary.version.as_ref().unwrap()).unwrap();
        newer.patch += 1;
        for version in [
            original.summary.version.clone(),
            original
                .summary
                .version
                .as_ref()
                .map(|version| format!("{version}+retained")),
            Some("99.0.0".into()),
            Some(newer.to_string()),
            Some("not-semver".into()),
            None,
        ] {
            let mut custom = original.clone();
            custom.summary.version = version;
            custom.summary.description = Some("Retained package metadata".into());
            save(&db, &custom, true).unwrap();
            install(&mut db, &host).unwrap();
            assert_eq!(required(&db, name).unwrap(), custom);
        }
        let removed = changed(&db, name, "0.0.1");
        save(&db, &removed, false).unwrap();
        install(&mut db, &host).unwrap();
        assert!(get(&db, name).unwrap().is_none());
        let retained: Vec<u8> = db
            .query_row("SELECT body FROM plugins WHERE name=?1", [name], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(retained, encode(&removed).unwrap());
    }
    assert!(get(&db, "plugin-builder").unwrap().is_none());
}

#[test]
fn skips_same_and_older_versions() {
    let (_directory, host, mut db) = fixture();
    let bundled = required(&db, "brief").unwrap();
    for version in [
        bundled.summary.version.clone().unwrap(),
        format!("{}+retained", bundled.summary.version.as_ref().unwrap()),
        "99.0.0".into(),
    ] {
        let previous = changed(&db, "brief", &version);
        let retained = captured(&db, &previous);
        let events: i64 = db
            .query_row("SELECT count(*) FROM events", [], |row| row.get(0))
            .unwrap();
        install(&mut db, &host).unwrap();
        assert_eq!(required(&db, "brief").unwrap(), previous);
        assert_eq!(captured(&db, &previous), retained);
        assert_eq!(
            db.query_row("SELECT count(*) FROM events", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            events
        );
    }
}

#[test]
fn refresh_rolls_back_and_remains_idempotent() {
    let (_directory, host, mut db) = fixture();
    let code = changed(&db, "code", "0.0.1");
    let search = changed(&db, "web-search", "0.0.1");
    let retained_code = captured(&db, &code);
    let retained_search = captured(&db, &search);
    let count = |db: &Connection| {
        db.query_row("SELECT count(*) FROM events", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap()
    };
    let before = count(&db);
    db.execute_batch("CREATE TEMP TRIGGER fail_refresh BEFORE UPDATE ON plugins WHEN NEW.name='web-search' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    assert!(install(&mut db, &host).is_err());
    assert_eq!(required(&db, "code").unwrap(), code);
    assert_eq!(required(&db, "web-search").unwrap(), search);
    assert_eq!(captured(&db, &code), retained_code);
    assert_eq!(captured(&db, &search), retained_search);
    assert_eq!(count(&db), before);
    db.execute_batch("DROP TRIGGER fail_refresh").unwrap();
    install(&mut db, &host).unwrap();
    let refreshed_code = required(&db, "code").unwrap();
    let refreshed_search = required(&db, "web-search").unwrap();
    assert_eq!(refreshed_code.summary.revision, code.summary.revision + 1);
    assert_eq!(
        refreshed_search.summary.revision,
        search.summary.revision + 1
    );
    assert_eq!(captured(&db, &code), retained_code);
    assert_eq!(captured(&db, &search), retained_search);
    assert_eq!(count(&db), before + 2);
    install(&mut db, &host).unwrap();
    assert_eq!(required(&db, "code").unwrap(), refreshed_code);
    assert_eq!(required(&db, "web-search").unwrap(), refreshed_search);
    assert_eq!(captured(&db, &code), retained_code);
    assert_eq!(captured(&db, &search), retained_search);
    assert_eq!(count(&db), before + 2);
}

#[test]
fn failure_rolls_back_inventory_and_initialization_marker() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().canonicalize().unwrap();
    let host = crate::plugins::Host::new(Some(profile));
    let mut db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../../schema.sql")).unwrap();
    db.execute(
        "INSERT INTO node(singleton,identity) VALUES(1,?1)",
        [&[1u8; 32][..]],
    )
    .unwrap();
    db.execute_batch("CREATE TEMP TRIGGER fail_install BEFORE INSERT ON plugins WHEN NEW.name='external-browser' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    assert!(install(&mut db, &host).is_err());
    let state = |db: &Connection| {
        db.query_row("SELECT plugin_defaults,(SELECT count(*) FROM plugins),(SELECT count(*) FROM events) FROM node", [], |row| Ok((row.get::<_, bool>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))).unwrap()
    };
    assert_eq!(state(&db), (false, 0, 0));
    db.execute_batch("DROP TRIGGER fail_install").unwrap();
    install(&mut db, &host).unwrap();
    let count = crate::plugins::DEFAULTS.len() as i64;
    assert_eq!(state(&db), (true, count, count));
    for &(name, enabled) in crate::plugins::DEFAULTS {
        let info = required(&db, name).unwrap();
        assert_eq!(info.summary.enabled, enabled, "{name}");
        assert_eq!(info.summary.revision, 1, "{name}");
    }
    let mut query = db
        .prepare("SELECT body FROM events ORDER BY cursor")
        .unwrap();
    let events = query
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .unwrap()
        .map(|body| serde_json::from_slice::<Event>(&body.unwrap()).unwrap())
        .collect::<Vec<_>>();
    for event in events {
        let Event::PluginChanged(summary) = event else {
            panic!("plugin change expected")
        };
        assert_eq!(required(&db, &summary.name).unwrap().summary, summary);
    }
    drop(query);
    install(&mut db, &host).unwrap();
    assert_eq!(state(&db), (true, count, count));
    assert!(!required(&db, "external-browser").unwrap().summary.enabled);
}

#[test]
fn binds_services_without_package_names() {
    let (_directory, _host, db) = fixture();
    let mut code = required(&db, "code").unwrap();
    let mut custom = code.clone();
    custom.summary.name = "language-tools".into();
    assert!(services::check(&db, &custom).is_err());
    code.extension.as_mut().unwrap().enabled_service = None;
    save(&db, &code, true).unwrap();
    services::check(&db, &custom).unwrap();
    save(&db, &custom, true).unwrap();
    services::apply(&db, &custom, false).unwrap();
    assert!(!crate::store::code::settings(&db).unwrap().enabled);
    let changed = services::synchronize(&db, plugin::Service::Language, false)
        .unwrap()
        .unwrap();
    assert_eq!(changed.name, "language-tools");
    assert!(required(&db, "code").unwrap().summary.enabled);
    assert!(!required(&db, "language-tools").unwrap().summary.enabled);
    save(&db, &custom, false).unwrap();
    assert_eq!(
        services::synchronize(&db, plugin::Service::Language, true)
            .unwrap_err()
            .code,
        ErrorCode::NotConfigured
    );
}
