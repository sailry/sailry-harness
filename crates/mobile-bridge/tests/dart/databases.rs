#[path = "../../../node-runtime/tests/databases/servers.rs"]
#[expect(
    dead_code,
    reason = "The shared Node fixture also exposes Unix sockets exercised by database tests"
)]
mod servers;

#[test]
#[ignore = "requires built bridge, Dart, PostgreSQL and MySQL executables"]
fn controls_execution_owned_databases() {
    use sailry_protocol::database::{Connection, Engine};
    let directory = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let node = runtime
        .block_on(sailry_node_runtime::Node::start(
            directory.path().join("node"),
        ))
        .unwrap();
    let sqlite = directory.path().join("fixture.sqlite3");
    rusqlite::Connection::open(&sqlite).unwrap();
    let postgres = runtime.block_on(servers::Server::start(Engine::Postgres));
    let mysql = runtime.block_on(servers::Server::start(Engine::Mysql));
    let connections = [
        Connection::Sqlite {
            path: sqlite.to_str().unwrap().into(),
        },
        postgres.connection.clone(),
        mysql.connection.clone(),
    ];
    let profiles: Vec<_> = connections
        .into_iter()
        .map(|connection| {
            serde_json::json!({
                "id": sailry_protocol::DatabaseId::new(),
                "revision": 0,
                "name": "FFI database",
                "connection": connection,
                "read_only": false,
            })
        })
        .collect();
    let invitation = node.link().invite().unwrap();
    let child = super::command("databases.dart", directory.path(), invitation.ticket())
        .env(
            "SAILRY_DATABASE_PROFILES",
            serde_json::to_string(&profiles).unwrap(),
        )
        .env("SAILRY_DATABASE_PASSWORD", servers::PASSWORD)
        .spawn()
        .unwrap();
    let status = super::wait(child);
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
}
