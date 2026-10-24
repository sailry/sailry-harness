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

async fn save(db: &Connection, profile: Profile, password: Option<&str>) -> Result<Profile, Fault> {
    let revision = profile.revision;
    let (Output::DatabaseProfile(profile), _) = super::super::execute(
        db,
        &Command::SaveDatabase {
            profile,
            expected_revision: revision,
            password: password.map(|value| Secret::new(value.into())),
        },
    )
    .await?
    else {
        panic!("database profile expected")
    };
    Ok(profile)
}

async fn database() -> (tempfile::TempDir, Database) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().canonicalize().unwrap().join("node.turso");
    let db = Database::open(&path, NodeId([12; 32]), None).await.unwrap();
    (directory, db)
}

#[tokio::test]
async fn preserves_names_and_rejects_changed_destinations() {
    let (_directory, database) = database().await;
    let db = &database.connection;
    let original = save(db, profile(), Some("private-password")).await.unwrap();
    let mut renamed = original.clone();
    renamed.name = "Renamed".into();
    renamed.read_only = false;
    let current = save(db, renamed, None).await.unwrap();
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
            save(db, changed.clone(), None).await.unwrap_err().code,
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
        .await
        .err()
        .expect("draft must not reuse the password");
        assert_eq!(error.code, ErrorCode::NotConfigured, "{field}");
        assert_eq!(
            super::super::read(db, current.id).await.unwrap(),
            Some(current.clone())
        );
    }
    let mut changed = current.clone();
    if let Target::Network { host, .. } = &mut changed.connection {
        *host = "replacement.test".into();
    }
    let replaced = save(db, changed, Some("replacement-password"))
        .await
        .unwrap();
    check(db, replaced.id, &replaced.connection).await.unwrap();
    let password: String = (async {
        let mut statement = db.prepare("SELECT password FROM database_profiles").await?;
        let row = statement.query_row(()).await?;
        let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
        mapped
    })
    .await
    .unwrap();
    assert_eq!(password, "replacement-password");
}

#[tokio::test]
async fn resolves_tunnel_authority_instead_of_ssh_id() {
    let (_directory, database) = database().await;
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
    .await
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
    let saved = save(db, target, Some("private-password")).await.unwrap();
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
    .await
    .unwrap();
    check(db, saved.id, &saved.connection).await.unwrap();
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
        .await
        .unwrap();
        assert_eq!(
            check(db, saved.id, &saved.connection)
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured,
            "{field}"
        );
    }
    let password: String = (async {
        let mut statement = db.prepare("SELECT password FROM database_profiles").await?;
        let row = statement.query_row(()).await?;
        let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
        mapped
    })
    .await
    .unwrap();
    assert_eq!(password, "private-password");
}

#[tokio::test]
async fn defaults_only_self_describing_legacy_targets() {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(include_str!("../../schema.sql"))
        .await
        .unwrap();
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
        .await
        .unwrap();
    }
    assert!(reconcile(&db).await.unwrap());
    assert!(!reconcile(&db).await.unwrap());
    check(&db, direct.id, &direct.connection).await.unwrap();
    let unbound: Option<Vec<u8>> = (async {
        let mut statement = db
            .prepare("SELECT password_target FROM database_profiles WHERE id=?1")
            .await?;
        let row = statement.query_row([tunnel.id.to_string()]).await?;
        let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
        mapped
    })
    .await
    .unwrap();
    assert!(unbound.is_none());
    let passwords: i64 = (async {
        let mut statement = db
            .prepare("SELECT count(*) FROM database_profiles WHERE password='preserved'")
            .await?;
        let row = statement.query_row(()).await?;
        let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
        mapped
    })
    .await
    .unwrap();
    assert_eq!(passwords, 2);
    validate(&db).await.unwrap();
}

#[tokio::test]
async fn upgrades_on_private_snapshot_and_preserves_passwords() {
    let (directory, database) = database().await;
    let path = directory.path().canonicalize().unwrap().join("node.turso");
    let original = save(&database.connection, profile(), Some("legacy-password"))
        .await
        .unwrap();
    database.close().await.unwrap();
    let source_handle = crate::store::database::engine(&path).await.unwrap();
    let source = crate::store::database::connect(&source_handle)
        .await
        .unwrap();
    source
        .execute_batch("ALTER TABLE database_profiles DROP COLUMN password_target")
        .await
        .unwrap();
    drop(source);
    drop(source_handle);
    let upgraded = Database::open(&path, NodeId([12; 32]), None).await.unwrap();
    check(&upgraded.connection, original.id, &original.connection)
        .await
        .unwrap();
    let password: String = (async {
        let mut statement = upgraded
            .connection
            .prepare("SELECT password FROM database_profiles")
            .await?;
        let row = statement.query_row(()).await?;
        let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
        mapped
    })
    .await
    .unwrap();
    assert_eq!(password, "legacy-password");
    upgraded.close().await.unwrap();
    let backups = std::fs::read_dir(directory.path().join("backups"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    let archived = family(&backups[0].join("node.turso"));
    let backup_handle = turso::Builder::new_local(backups[0].join("node.turso").to_str().unwrap())
        .experimental_index_method(true)
        .read_only(true)
        .build()
        .await
        .unwrap();
    let backup = crate::store::database::connect(&backup_handle)
        .await
        .unwrap();
    let missing:bool=(async { let mut statement = backup.prepare("SELECT NOT EXISTS(SELECT 1 FROM pragma_table_info('database_profiles') WHERE name='password_target')").await?;
let row = statement.query_row(()).await?;
let mapped: turso::Result<_> = (|row: &turso::Row|row.get(0))(&row);
mapped }).await.unwrap();
    assert!(missing);
    assert_eq!(
        (async {
            let mut statement = backup
                .prepare("SELECT password FROM database_profiles")
                .await?;
            let row = statement.query_row(()).await?;
            let mapped: turso::Result<_> = (|row: &turso::Row| row.get::<String>(0))(&row);
            mapped
        })
        .await
        .unwrap(),
        "legacy-password"
    );
    drop(backup);
    drop(backup_handle);
    assert_eq!(family(&backups[0].join("node.turso")), archived);
    let source_handle = crate::store::database::engine(&path).await.unwrap();
    let source = crate::store::database::connect(&source_handle)
        .await
        .unwrap();
    source
        .execute(
            "UPDATE database_profiles SET password_target=?1",
            [b"invalid".as_slice()],
        )
        .await
        .unwrap();
    drop(source);
    drop(source_handle);
    let before = family(&path);
    assert!(Database::open(&path, NodeId([12; 32]), None).await.is_err());
    assert_eq!(family(&path), before);
    let snapshots: std::collections::BTreeSet<_> =
        std::fs::read_dir(directory.path().join("backups"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_dir())
            .collect();
    assert_eq!(snapshots, backups.into_iter().collect());
}

#[tokio::test]
async fn binds_socket_passwords_to_the_socket_path() {
    let (directory, database) = database().await;
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
    let saved = save(&database.connection, draft, Some("socket-password"))
        .await
        .unwrap();
    let mut changed = saved.clone();
    if let Target::Socket { path, .. } = &mut changed.connection {
        *path = directory.path().join("other.sock").to_str().unwrap().into();
    }
    assert_eq!(
        save(&database.connection, changed, None)
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotConfigured
    );
    check(&database.connection, saved.id, &saved.connection)
        .await
        .unwrap();
}

fn family(path: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    // The main file and MVCC log are required; the remaining sidecars are optional.
    crate::profile::database_files(path)
        .into_iter()
        .enumerate()
        .filter_map(|(index, file)| match std::fs::read(&file) {
            Ok(bytes) => Some((
                file.file_name().unwrap().to_string_lossy().into_owned(),
                bytes,
            )),
            Err(error) if index >= 2 && error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("fixture database family read failed: {error}"),
        })
        .collect()
}
