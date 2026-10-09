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

fn older(db: &Connection, name: &str) -> Info {
    let mut info = required(db, name).unwrap();
    info.summary.version = Some("0.0.1".into());
    info.extension.as_mut().unwrap().settings_page = None;
    save(db, &info, true).unwrap();
    info
}

#[test]
fn refresh_preserves_configuration_and_private_records() {
    let (_directory, host, mut db) = fixture();
    let mut before = older(&db, "code");
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
        after.extension.unwrap().settings_page.unwrap().native,
        Some(plugin::desktop::NativeSettings::Code)
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
fn skips_custom_newer_unchanged_and_removed_packages() {
    let (_directory, host, mut db) = fixture();
    for name in ["code", "web-search"] {
        let original = required(&db, name).unwrap();
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
            let mut custom = original.clone();
            custom.summary.version = Some("0.0.1".into());
            custom.origin = origin;
            save(&db, &custom, true).unwrap();
            install(&mut db, &host).unwrap();
            assert_eq!(required(&db, name).unwrap(), custom);
        }
        for version in [
            original.summary.version.clone(),
            original
                .summary
                .version
                .as_ref()
                .map(|version| format!("{version}+retained")),
            Some("99.0.0".into()),
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
        let removed = older(&db, name);
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
    let memory = older(&db, "memory");
    install(&mut db, &host).unwrap();
    assert_eq!(required(&db, "memory").unwrap(), memory);
}

#[test]
fn refresh_rolls_back_and_remains_idempotent() {
    let (_directory, host, mut db) = fixture();
    let code = older(&db, "code");
    let search = older(&db, "web-search");
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
    assert_eq!(count(&db), before);
    db.execute_batch("DROP TRIGGER fail_refresh").unwrap();
    install(&mut db, &host).unwrap();
    let code = required(&db, "code").unwrap();
    let search = required(&db, "web-search").unwrap();
    assert_eq!(count(&db), before + 2);
    install(&mut db, &host).unwrap();
    assert_eq!(required(&db, "code").unwrap(), code);
    assert_eq!(required(&db, "web-search").unwrap(), search);
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
