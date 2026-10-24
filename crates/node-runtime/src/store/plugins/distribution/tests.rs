use super::*;

async fn fixture() -> (tempfile::TempDir, crate::plugins::Host, Connection) {
    let directory = tempfile::tempdir().unwrap();
    let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let mut db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(include_str!("../../schema.sql"))
        .await
        .unwrap();
    db.execute(
        "INSERT INTO node(singleton,identity) VALUES(1,?1)",
        [&[1u8; 32][..]],
    )
    .await
    .unwrap();
    install(&mut db, &host).await.unwrap();
    (directory, host, db)
}

async fn changed(db: &Connection, name: &str, version: &str) -> Info {
    let mut info = required(db, name).await.unwrap();
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
        params![
            package.summary.digest.as_str(),
            name,
            encode(&package).unwrap()
        ],
    )
    .await
    .unwrap();
    save(db, &info, true).await.unwrap();
    info
}

async fn captured(db: &Connection, info: &Info) -> Info {
    let (Output::Plugin(package), _) = execute(
        db,
        None,
        &Command::ReadPluginVersion {
            package: info.summary.reference(),
        },
    )
    .await
    .unwrap() else {
        panic!("captured plugin expected")
    };
    package
}

#[tokio::test]
async fn adds_missing_defaults_without_reinstalling_records() {
    let (_directory, host, mut db) = fixture().await;
    let mut memory = required(&db, "memory").await.unwrap();
    memory.summary.enabled = false;
    save(&db, &memory, true).await.unwrap();
    let search = required(&db, "web-search").await.unwrap();
    save(&db, &search, false).await.unwrap();
    db.execute("DELETE FROM plugins WHERE name='code'", ())
        .await
        .unwrap();
    let mut settings = crate::store::code::settings(&db).await.unwrap();
    settings.enabled = false;
    settings.after_edit = false;
    let settings = crate::store::code::save(&db, &settings, settings.revision)
        .await
        .unwrap();
    let before = list(&db).await.unwrap();
    let events: i64 = (async {
        let mut statement = db.prepare("SELECT count(*) FROM events").await?;
        let row = statement.query_row(()).await?;
        let mapped: turso::Result<_> = row.get(0);
        mapped
    })
    .await
    .unwrap();
    db.execute_batch("CREATE TEMP TRIGGER fail_default BEFORE INSERT ON plugins WHEN NEW.name='code' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").await.unwrap();
    assert!(install(&mut db, &host).await.is_err());
    assert_eq!(list(&db).await.unwrap(), before);
    assert_eq!(
        (async {
            let mut statement = db.prepare("SELECT count(*) FROM events").await?;
            let row = statement.query_row(()).await?;
            let mapped: turso::Result<_> = row.get::<i64>(0);
            mapped
        })
        .await
        .unwrap(),
        events
    );
    db.execute_batch("DROP TRIGGER fail_default").await.unwrap();
    install(&mut db, &host).await.unwrap();
    let code = required(&db, "code").await.unwrap();
    assert!(!code.summary.enabled);
    assert_eq!(crate::store::code::settings(&db).await.unwrap(), settings);
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
    assert_eq!(required(&db, "memory").await.unwrap(), memory);
    assert!(get(&db, "web-search").await.unwrap().is_none());
    let mut after = list(&db).await.unwrap();
    after.retain(|summary| summary.name != "code");
    assert_eq!(after, before);
    let recorded: Vec<u8> = (async {
        let mut statement = db
            .prepare("SELECT body FROM plugins WHERE name='web-search'")
            .await?;
        let row = statement.query_row(()).await?;
        let mapped: turso::Result<_> = row.get(0);
        mapped
    })
    .await
    .unwrap();
    assert_eq!(recorded, encode(&search).unwrap());
    let installed = list(&db).await.unwrap();
    install(&mut db, &host).await.unwrap();
    assert_eq!(list(&db).await.unwrap(), installed);
    assert_eq!(
        (async {
            let mut statement = db.prepare("SELECT count(*) FROM events").await?;
            let row = statement.query_row(()).await?;
            let mapped: turso::Result<_> = row.get::<i64>(0);
            mapped
        })
        .await
        .unwrap(),
        events + 1
    );
}

#[tokio::test]
async fn refresh_preserves_configuration_and_private_records() {
    let (_directory, host, mut db) = fixture().await;
    let mut before = changed(&db, "code", "0.0.1").await;
    before.summary.enabled = false;
    before.summary.settings_revision = 7;
    save(&db, &before, true).await.unwrap();
    let settings = serde_json::json!({
        "schema":null,"values":{"draft":"retained", "model":"fixture/model"},
        "slots":{},"authorizations":{}
    });
    let body = serde_json::to_vec(&settings).unwrap();
    db.execute(
        "INSERT INTO plugin_settings(plugin,revision,body) VALUES('code',7,?1)",
        [body.as_slice()],
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO plugin_values(name,key,revision,value) VALUES('code','draft',1,?1)",
        [&b"{\"text\":\"retained\"}"[..]],
    )
    .await
    .unwrap();
    let retained = captured(&db, &before).await;
    install(&mut db, &host).await.unwrap();
    let after = required(&db, "code").await.unwrap();
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
    assert_eq!(captured(&db, &before).await, retained);
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
    let saved: Vec<u8> = (async {
        let mut statement = db
            .prepare("SELECT body FROM plugin_settings WHERE plugin='code' AND revision=7")
            .await?;
        let row = statement.query_row(()).await?;
        let mapped: turso::Result<_> = row.get(0);
        mapped
    })
    .await
    .unwrap();
    assert_eq!(saved, body);
    let value: Vec<u8> = (async {
        let mut statement = db
            .prepare("SELECT value FROM plugin_values WHERE name='code' AND key='draft'")
            .await?;
        let row = statement.query_row(()).await?;
        let mapped: turso::Result<_> = row.get(0);
        mapped
    })
    .await
    .unwrap();
    assert_eq!(value, b"{\"text\":\"retained\"}");
}

#[tokio::test]
async fn skips_custom_unchanged_and_removed_packages() {
    let (_directory, host, mut db) = fixture().await;
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
        let original = required(&db, name).await.unwrap();
        let previous = changed(&db, name, "0.0.1").await;
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
            save(&db, &custom, true).await.unwrap();
            install(&mut db, &host).await.unwrap();
            assert_eq!(required(&db, name).await.unwrap(), custom);
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
            save(&db, &custom, true).await.unwrap();
            install(&mut db, &host).await.unwrap();
            assert_eq!(required(&db, name).await.unwrap(), custom);
        }
        let removed = changed(&db, name, "0.0.1").await;
        save(&db, &removed, false).await.unwrap();
        install(&mut db, &host).await.unwrap();
        assert!(get(&db, name).await.unwrap().is_none());
        let retained: Vec<u8> = (async {
            let mut statement = db.prepare("SELECT body FROM plugins WHERE name=?1").await?;
            let row = statement.query_row([name]).await?;
            let mapped: turso::Result<_> = row.get(0);
            mapped
        })
        .await
        .unwrap();
        assert_eq!(retained, encode(&removed).unwrap());
    }
    assert!(get(&db, "plugin-builder").await.unwrap().is_none());
}

#[tokio::test]
async fn skips_same_and_older_versions() {
    let (_directory, host, mut db) = fixture().await;
    let bundled = required(&db, "brief").await.unwrap();
    for version in [
        bundled.summary.version.clone().unwrap(),
        format!("{}+retained", bundled.summary.version.as_ref().unwrap()),
        "99.0.0".into(),
    ] {
        let previous = changed(&db, "brief", &version).await;
        let retained = captured(&db, &previous).await;
        let events: i64 = (async {
            let mut statement = db.prepare("SELECT count(*) FROM events").await?;
            let row = statement.query_row(()).await?;
            let mapped: turso::Result<_> = row.get(0);
            mapped
        })
        .await
        .unwrap();
        install(&mut db, &host).await.unwrap();
        assert_eq!(required(&db, "brief").await.unwrap(), previous);
        assert_eq!(captured(&db, &previous).await, retained);
        assert_eq!(
            (async {
                let mut statement = db.prepare("SELECT count(*) FROM events").await?;
                let row = statement.query_row(()).await?;
                let mapped: turso::Result<_> = row.get::<i64>(0);
                mapped
            })
            .await
            .unwrap(),
            events
        );
    }
}

#[tokio::test]
async fn refresh_rolls_back_and_remains_idempotent() {
    let (_directory, host, mut db) = fixture().await;
    let code = changed(&db, "code", "0.0.1").await;
    let search = changed(&db, "web-search", "0.0.1").await;
    let retained_code = captured(&db, &code).await;
    let retained_search = captured(&db, &search).await;
    let count = async |db: &Connection| {
        (async {
            let mut statement = db.prepare("SELECT count(*) FROM events").await?;
            let row = statement.query_row(()).await?;
            let mapped: turso::Result<_> = row.get::<i64>(0);
            mapped
        })
        .await
        .unwrap()
    };
    let before = count(&db).await;
    db.execute_batch("CREATE TEMP TRIGGER fail_refresh BEFORE UPDATE ON plugins WHEN NEW.name='web-search' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").await.unwrap();
    assert!(install(&mut db, &host).await.is_err());
    assert_eq!(required(&db, "code").await.unwrap(), code);
    assert_eq!(required(&db, "web-search").await.unwrap(), search);
    assert_eq!(captured(&db, &code).await, retained_code);
    assert_eq!(captured(&db, &search).await, retained_search);
    assert_eq!(count(&db).await, before);
    db.execute_batch("DROP TRIGGER fail_refresh").await.unwrap();
    install(&mut db, &host).await.unwrap();
    let refreshed_code = required(&db, "code").await.unwrap();
    let refreshed_search = required(&db, "web-search").await.unwrap();
    assert_eq!(refreshed_code.summary.revision, code.summary.revision + 1);
    assert_eq!(
        refreshed_search.summary.revision,
        search.summary.revision + 1
    );
    assert_eq!(captured(&db, &code).await, retained_code);
    assert_eq!(captured(&db, &search).await, retained_search);
    assert_eq!(count(&db).await, before + 2);
    install(&mut db, &host).await.unwrap();
    assert_eq!(required(&db, "code").await.unwrap(), refreshed_code);
    assert_eq!(required(&db, "web-search").await.unwrap(), refreshed_search);
    assert_eq!(captured(&db, &code).await, retained_code);
    assert_eq!(captured(&db, &search).await, retained_search);
    assert_eq!(count(&db).await, before + 2);
}

#[tokio::test]
async fn failure_rolls_back_inventory_and_initialization_marker() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().canonicalize().unwrap();
    let host = crate::plugins::Host::new(Some(profile));
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let mut db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(include_str!("../../schema.sql"))
        .await
        .unwrap();
    db.execute(
        "INSERT INTO node(singleton,identity) VALUES(1,?1)",
        [&[1u8; 32][..]],
    )
    .await
    .unwrap();
    db.execute_batch("CREATE TEMP TRIGGER fail_install BEFORE INSERT ON plugins WHEN NEW.name='external-browser' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").await.unwrap();
    assert!(install(&mut db, &host).await.is_err());
    let state = async |db: &Connection| {
        (async { let mut statement = db.prepare("SELECT plugin_defaults,(SELECT count(*) FROM plugins),(SELECT count(*) FROM events) FROM node").await?;
let row = statement.query_row(()).await?;
let mapped: turso::Result<_> = (|row: &turso::Row| Ok((row.get::<bool>(0)?, row.get::<i64>(1)?, row.get::<i64>(2)?)))(&row);
mapped }).await.unwrap()
    };
    assert_eq!(state(&db).await, (false, 0, 0));
    db.execute_batch("DROP TRIGGER fail_install").await.unwrap();
    install(&mut db, &host).await.unwrap();
    let count = crate::plugins::DEFAULTS.len() as i64;
    assert_eq!(state(&db).await, (true, count, count));
    for &(name, enabled) in crate::plugins::DEFAULTS {
        let info = required(&db, name).await.unwrap();
        assert_eq!(info.summary.enabled, enabled, "{name}");
        assert_eq!(info.summary.revision, 1, "{name}");
    }
    let mut query = db
        .prepare("SELECT body FROM events ORDER BY cursor")
        .await
        .unwrap();
    let events = (async {
        let mut rows = query.query(()).await?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await? {
            let mapped: turso::Result<_> = row.get::<Vec<u8>>(0);
            values.push(mapped);
        }
        Ok::<_, turso::Error>(values.into_iter())
    })
    .await
    .unwrap()
    .map(|body| serde_json::from_slice::<Event>(&body.unwrap()).unwrap())
    .collect::<Vec<_>>();
    for event in events {
        let Event::PluginChanged(summary) = event else {
            panic!("plugin change expected")
        };
        assert_eq!(required(&db, &summary.name).await.unwrap().summary, summary);
    }
    drop(query);
    install(&mut db, &host).await.unwrap();
    assert_eq!(state(&db).await, (true, count, count));
    assert!(
        !required(&db, "external-browser")
            .await
            .unwrap()
            .summary
            .enabled
    );
}

#[tokio::test]
async fn binds_services_without_package_names() {
    let (_directory, _host, db) = fixture().await;
    let mut code = required(&db, "code").await.unwrap();
    let mut custom = code.clone();
    custom.summary.name = "language-tools".into();
    assert!(services::check(&db, &custom).await.is_err());
    code.extension.as_mut().unwrap().enabled_service = None;
    save(&db, &code, true).await.unwrap();
    services::check(&db, &custom).await.unwrap();
    save(&db, &custom, true).await.unwrap();
    services::apply(&db, &custom, false).await.unwrap();
    assert!(!crate::store::code::settings(&db).await.unwrap().enabled);
    let changed = services::synchronize(&db, plugin::Service::Language, false)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(changed.name, "language-tools");
    assert!(required(&db, "code").await.unwrap().summary.enabled);
    assert!(
        !required(&db, "language-tools")
            .await
            .unwrap()
            .summary
            .enabled
    );
    save(&db, &custom, false).await.unwrap();
    assert_eq!(
        services::synchronize(&db, plugin::Service::Language, true)
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotConfigured
    );
}
