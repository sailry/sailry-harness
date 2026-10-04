use super::*;

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
