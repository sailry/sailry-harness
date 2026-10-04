use super::*;
use sailry_protocol::{
    connection::{Resource, Sharing},
    database,
};

async fn profile(client: &Client, path: &Path, shared: bool) -> database::Profile {
    rusqlite::Connection::open(path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE effects(value INTEGER); INSERT INTO effects VALUES(9223372036854775807)",
        )
        .unwrap();
    let Output::DatabaseProfile(profile) = execute(
        client,
        Command::SaveDatabase {
            profile: database::Profile {
                id: DatabaseId::new(),
                revision: 0,
                name: "Scoped SQLite".into(),
                connection: database::Connection::Sqlite {
                    path: path.to_str().unwrap().into(),
                },
                read_only: false,
                sharing: shared.then_some(Sharing::Global),
            },
            expected_revision: 0,
            password: None,
        },
    )
    .await
    else {
        panic!("database profile expected")
    };
    profile
}
fn query(profile: &database::Profile, sql: &str, read_only: bool) -> Command {
    Command::QueryDatabase {
        profile: profile.id,
        expected_revision: profile.revision,
        database: None,
        sql: sql.into(),
        read_only,
        row_limit: 1000,
        timeout_ms: 30_000,
    }
}
async fn scoped(client: &Client, context: &Context, command: Command) -> Result<Output, Fault> {
    client
        .execute(client.prepare(command).with_plugin(context.clone()))
        .await
}

#[tokio::test]
async fn confines_connection_resources() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let mut context = install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::ReadDatabases],
        )
        .await;
        let shared = profile(&client, &directory.path().join("shared.sqlite3"), true).await;
        let private = profile(&client, &directory.path().join("private.sqlite3"), false).await;
        let Output::DatabaseProfiles(profiles) = scoped(&client, &context, Command::ListDatabases)
            .await
            .unwrap()
        else {
            panic!("profiles expected")
        };
        assert_eq!(profiles, vec![shared.clone()]);
        let Output::DatabaseOutcome(database::Outcome::Query(result)) = scoped(
            &client,
            &context,
            query(&shared, "SELECT value FROM effects", true),
        )
        .await
        .unwrap() else {
            panic!("query expected")
        };
        assert_eq!(result.rows, vec![vec![database::Value::Integer(i64::MAX)]]);
        assert_eq!(
            scoped(
                &client,
                &context,
                query(&shared, "DELETE FROM effects", false)
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(
            scoped(
                &client,
                &context,
                query(&private, "SELECT value FROM effects", true)
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::PermissionDenied
        );
        let session = scopes::session(&client, worktree).await;
        let Output::Session(session) = execute(
            &client,
            Command::BindConnectionSession {
                session: session.id,
                expected_revision: session.revision,
                resource: Resource::Database(private.id),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        context.session = Some(session.id);
        context.worktree = Some(session.worktree);
        let Output::DatabaseProfiles(profiles) = scoped(&client, &context, Command::ListDatabases)
            .await
            .unwrap()
        else {
            panic!("profiles expected")
        };
        assert_eq!(profiles, vec![private.clone()]);
        assert!(
            scoped(
                &client,
                &context,
                query(&private, "SELECT value FROM effects", true)
            )
            .await
            .is_ok()
        );
        assert_eq!(
            scoped(
                &client,
                &context,
                query(&shared, "SELECT value FROM effects", true)
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(
            rusqlite::Connection::open(directory.path().join("shared.sqlite3"))
                .unwrap()
                .query_row("SELECT COUNT(*) FROM effects", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn recovers_database_receipts_without_replaying_effects() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let context = install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::ReadDatabases, Action::ControlDatabases],
        )
        .await;
        let profile = profile(&client, &directory.path().join("data.sqlite3"), true).await;
        let request = client
            .prepare(query(&profile, "INSERT INTO effects VALUES(7)", false))
            .with_plugin(context.clone());
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        drop(admission);
        let completed = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let RequestOutcome::Completed(result) = client.outcome(&request).await.unwrap() {
                    break result.unwrap();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(client.execute(request.clone()).await.unwrap(), completed);
        let mut changed = request.clone();
        changed.command = query(&profile, "INSERT INTO effects VALUES(8)", false);
        assert_eq!(
            client.execute(changed).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        execute(
            &client,
            Command::SetPluginEnabled {
                name: context.package.name.clone(),
                expected_revision: 1,
                enabled: false,
            },
        )
        .await;
        assert_eq!(client.execute(request).await.unwrap(), completed);
        assert_eq!(
            scoped(
                &client,
                &context,
                query(&profile, "INSERT INTO effects VALUES(9)", false)
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::NotConfigured
        );
        let db = rusqlite::Connection::open(directory.path().join("data.sqlite3")).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM effects WHERE value=7", [], |row| row
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM effects", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn cancellation_keeps_the_original_plugin_scope() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let context = install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::ReadDatabases],
        )
        .await;
        let profile = profile(&client, &directory.path().join("cancel.sqlite3"), true).await;
        let request = client.prepare(query(&profile,"WITH RECURSIVE numbers(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM numbers WHERE x<100000000) SELECT SUM(x) FROM numbers",true)).with_plugin(context.clone());
        let admission = client.dispatch(request.clone()).await.unwrap();
        let mut other = context.clone();
        other.worktree = None;
        assert_eq!(
            scoped(
                &client,
                &other,
                Command::CancelDatabase {
                    request: request.id
                }
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(
            scoped(
                &client,
                &context,
                Command::CancelDatabase {
                    request: request.id
                }
            )
            .await
            .unwrap(),
            Output::DatabaseCancelled {
                request: request.id
            }
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(10), admission.completion)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
