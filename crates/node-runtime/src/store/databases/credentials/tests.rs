use super::*;
use sailry_protocol::database::Tls;

fn profile() -> Profile {
    Profile {
        sharing: None,
        id: DatabaseId::new(),
        revision: 0,
        name: "Database".into(),
        connection: Target::Network {
            engine: Engine::Postgres,
            host: "database.test".into(),
            port: 5432,
            database: "project".into(),
            username: "reader".into(),
            tls: Tls::Require,
        },
        read_only: true,
    }
}

fn save(db: &Connection, profile: Profile, password: Option<&str>) -> Result<Profile, Fault> {
    let revision = profile.revision;
    let (Output::DatabaseProfile(profile), _) = super::super::execute(
        db,
        &Command::SaveDatabase {
            profile,
            expected_revision: revision,
            password: password.map(|value| Secret::new(value.into())),
        },
    )?
    else {
        panic!("database profile expected")
    };
    Ok(profile)
}

fn database() -> (tempfile::TempDir, Database) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .canonicalize()
        .unwrap()
        .join("node.sqlite3");
    let db = Database::open(&path, NodeId([12; 32]), None).unwrap();
    (directory, db)
}

#[test]
fn preserves_names_and_rejects_changed_destinations() {
    let (_directory, database) = database();
    let db = &database.connection;
    let original = save(db, profile(), Some("private-password")).unwrap();
    let mut renamed = original.clone();
    renamed.name = "Renamed".into();
    renamed.read_only = false;
    let current = save(db, renamed, None).unwrap();
    for field in ["host", "port", "username", "database", "engine", "tls"] {
        let mut changed = current.clone();
        let Target::Network {
            host,
            port,
            username,
            database: catalog,
            engine,
            tls,
        } = &mut changed.connection
        else {
            unreachable!()
        };
        match field {
            "host" => *host = "other.test".into(),
            "port" => *port += 1,
            "username" => *username = "other".into(),
            "database" => *catalog = "other".into(),
            "engine" => *engine = Engine::Mysql,
            "tls" => *tls = Tls::Disable,
            _ => unreachable!(),
        }
        assert_eq!(
            save(db, changed.clone(), None).unwrap_err().code,
            ErrorCode::NotConfigured,
            "{field}"
        );
        let error = super::super::prepare(
            &database,
            &Command::TestDatabase {
                profile: changed,
                expected_revision: current.revision,
                password: None,
            },
        )
        .err()
        .expect("draft must not reuse the password");
        assert_eq!(error.code, ErrorCode::NotConfigured, "{field}");
        assert_eq!(
            super::super::read(db, current.id).unwrap(),
            Some(current.clone())
        );
    }
    let mut changed = current.clone();
    if let Target::Network { host, .. } = &mut changed.connection {
        *host = "replacement.test".into();
    }
    let replaced = save(db, changed, Some("replacement-password")).unwrap();
    check(db, replaced.id, &replaced.connection).unwrap();
    let password: String = db
        .query_row("SELECT password FROM database_profiles", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(password, "replacement-password");
}

#[test]
fn resolves_tunnel_authority_instead_of_ssh_id() {
    let (_directory, database) = database();
    let db = &database.connection;
    let mut tunnel = ssh::Profile {
        sharing: None,
        id: SshId::new(),
        revision: 1,
        name: "Tunnel".into(),
        host: "gateway.test".into(),
        port: 22,
        username: "reader".into(),
        authentication: ssh::Authentication::Agent,
        host_key: None,
    };
    db.execute(
        "INSERT INTO ssh_profiles(id,body,credential) VALUES(?1,?2,?3)",
        params![
            tunnel.id.to_string(),
            encode(&tunnel).unwrap(),
            "{\"kind\":\"agent\"}"
        ],
    )
    .unwrap();
    let mut target = profile();
    target.connection = Target::Ssh {
        engine: Engine::Postgres,
        host: "database.test".into(),
        port: 5432,
        database: "project".into(),
        username: "reader".into(),
        tls: Tls::Require,
        ssh: tunnel.id,
    };
    let saved = save(db, target, Some("private-password")).unwrap();
    tunnel.name = "Renamed tunnel".into();
    tunnel.revision += 1;
    tunnel.host_key = Some(ssh::HostKey {
        algorithm: "ssh-ed25519".into(),
        fingerprint: "SHA256:trusted".into(),
    });
    db.execute(
        "UPDATE ssh_profiles SET body=?1",
        [encode(&tunnel).unwrap()],
    )
    .unwrap();
    check(db, saved.id, &saved.connection).unwrap();
    for field in ["host", "port", "username", "authentication"] {
        let mut changed = tunnel.clone();
        match field {
            "host" => changed.host = "other.test".into(),
            "port" => changed.port = 2222,
            "username" => changed.username = "other".into(),
            "authentication" => changed.authentication = ssh::Authentication::Password,
            _ => unreachable!(),
        }
        db.execute(
            "UPDATE ssh_profiles SET body=?1",
            [encode(&changed).unwrap()],
        )
        .unwrap();
        assert_eq!(
            check(db, saved.id, &saved.connection).unwrap_err().code,
            ErrorCode::NotConfigured,
            "{field}"
        );
    }
    let password: String = db
        .query_row("SELECT password FROM database_profiles", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(password, "private-password");
}

#[test]
fn defaults_only_self_describing_legacy_targets() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../../schema.sql")).unwrap();
    let direct = profile();
    let mut tunnel = profile();
    tunnel.connection = Target::Ssh {
        engine: Engine::Postgres,
        host: "database.test".into(),
        port: 5432,
        database: "project".into(),
        username: "reader".into(),
        tls: Tls::Require,
        ssh: SshId::new(),
    };
    for profile in [&direct, &tunnel] {
        db.execute(
            "INSERT INTO database_profiles(id,body,password) VALUES(?1,?2,'preserved')",
            params![profile.id.to_string(), encode(profile).unwrap()],
        )
        .unwrap();
    }
    assert!(reconcile(&db).unwrap());
    assert!(!reconcile(&db).unwrap());
    check(&db, direct.id, &direct.connection).unwrap();
    let unbound: Option<Vec<u8>> = db
        .query_row(
            "SELECT password_target FROM database_profiles WHERE id=?1",
            [tunnel.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(unbound.is_none());
    let passwords: i64 = db
        .query_row(
            "SELECT count(*) FROM database_profiles WHERE password='preserved'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(passwords, 2);
    validate(&db).unwrap();
}

#[test]
fn upgrades_on_private_snapshot_and_preserves_passwords() {
    let (directory, database) = database();
    let path = directory
        .path()
        .canonicalize()
        .unwrap()
        .join("node.sqlite3");
    let original = save(&database.connection, profile(), Some("legacy-password")).unwrap();
    database.close().unwrap();
    let source = Connection::open(&path).unwrap();
    source
        .execute_batch("ALTER TABLE database_profiles DROP COLUMN password_target")
        .unwrap();
    source.close().unwrap();
    let upgraded = Database::open(&path, NodeId([12; 32]), None).unwrap();
    check(&upgraded.connection, original.id, &original.connection).unwrap();
    let password: String = upgraded
        .connection
        .query_row("SELECT password FROM database_profiles", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(password, "legacy-password");
    upgraded.close().unwrap();
    let backups = std::fs::read_dir(directory.path().join("backups"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    let archived = std::fs::read(&backups[0]).unwrap();
    let backup =
        Connection::open_with_flags(&backups[0], rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let missing:bool=backup.query_row("SELECT NOT EXISTS(SELECT 1 FROM pragma_table_info('database_profiles') WHERE name='password_target')",[],|row|row.get(0)).unwrap();
    assert!(missing);
    assert_eq!(
        backup
            .query_row("SELECT password FROM database_profiles", [], |row| row
                .get::<_, String>(
                0
            ))
            .unwrap(),
        "legacy-password"
    );
    drop(backup);
    assert_eq!(std::fs::read(&backups[0]).unwrap(), archived);
    let source = Connection::open(&path).unwrap();
    source
        .execute(
            "UPDATE database_profiles SET password_target=?1",
            [b"invalid".as_slice()],
        )
        .unwrap();
    source
        .pragma_update(None, "journal_mode", "DELETE")
        .unwrap();
    source.close().unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(Database::open(&path, NodeId([12; 32]), None).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let snapshots: std::collections::BTreeSet<_> =
        std::fs::read_dir(directory.path().join("backups"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "sqlite3")
            })
            .collect();
    assert_eq!(snapshots, backups.into_iter().collect());
}

#[test]
fn binds_socket_passwords_to_the_socket_path() {
    let (directory, database) = database();
    let mut draft = profile();
    draft.connection = Target::Socket {
        engine: Engine::Postgres,
        path: directory
            .path()
            .join("original.sock")
            .to_str()
            .unwrap()
            .into(),
        port: 5432,
        database: "project".into(),
        username: "reader".into(),
    };
    let saved = save(&database.connection, draft, Some("socket-password")).unwrap();
    let mut changed = saved.clone();
    if let Target::Socket { path, .. } = &mut changed.connection {
        *path = directory.path().join("other.sock").to_str().unwrap().into();
    }
    assert_eq!(
        save(&database.connection, changed, None).unwrap_err().code,
        ErrorCode::NotConfigured
    );
    check(&database.connection, saved.id, &saved.connection).unwrap();
}
