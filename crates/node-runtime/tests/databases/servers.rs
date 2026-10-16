use sailry_protocol::database::{Connection, Engine, Tls};
use std::{
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub const PASSWORD: &str = "database-fixture-password";
pub struct Server {
    pub connection: Connection,
    child: Child,
    _directory: tempfile::TempDir,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Server {
    pub fn socket(&self) -> Connection {
        let Connection::Network {
            engine,
            port,
            database,
            username,
            ..
        } = &self.connection
        else {
            unreachable!()
        };
        let path = if *engine == Engine::Mysql {
            self._directory.path().join("mysql.sock")
        } else {
            self._directory.path().into()
        };
        Connection::Socket {
            engine: *engine,
            path: path.to_str().unwrap().into(),
            port: *port,
            database: database.clone(),
            username: username.clone(),
        }
    }

    #[allow(
        dead_code,
        reason = "The shared Mobile fixture starts servers without custom TLS certificates"
    )]
    pub async fn start(engine: Engine) -> Self {
        Self::start_with_tls(engine, None).await
    }

    pub async fn start_with_tls(engine: Engine, certificates: Option<&Path>) -> Self {
        let directory = tempfile::Builder::new()
            .prefix("sailry-db-")
            .tempdir_in("/tmp")
            .unwrap();
        let root = directory.path();
        let data = root.join("data");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let (child, username, database) = match engine {
            Engine::Postgres => {
                let password = root.join("password");
                std::fs::write(&password, PASSWORD).unwrap();
                run(Command::new("initdb")
                    .arg("-D")
                    .arg(&data)
                    .args([
                        "-U",
                        "fixture",
                        "--encoding=UTF8",
                        "--no-locale",
                        "--auth-host=scram-sha-256",
                        "--auth-local=trust",
                        "--pwfile",
                    ])
                    .arg(password));
                let mut command = Command::new("postgres");
                command
                    .arg("-D")
                    .arg(&data)
                    .args(["-h", "127.0.0.1", "-p", &port.to_string(), "-k"])
                    .arg(root);
                if let Some(certificates) = certificates {
                    command
                        .args(["-c", "ssl=on"])
                        .arg("-c")
                        .arg(format!(
                            "ssl_cert_file={}",
                            certificates.join("server.pem").display()
                        ))
                        .arg("-c")
                        .arg(format!(
                            "ssl_key_file={}",
                            certificates.join("server.key").display()
                        ));
                }
                let child = spawn(&mut command, root);
                (child, "fixture", "postgres")
            }
            Engine::Mysql => {
                run(Command::new("mysqld")
                    .arg("--no-defaults")
                    .arg("--initialize-insecure")
                    .arg(format!("--datadir={}", data.display())));
                let mut command = Command::new("mysqld");
                command.arg("--no-defaults").args([
                    format!("--datadir={}", data.display()),
                    "--bind-address=127.0.0.1".into(),
                    format!("--port={port}"),
                    format!("--socket={}", root.join("mysql.sock").display()),
                    format!("--pid-file={}", root.join("mysql.pid").display()),
                    "--mysqlx=0".into(),
                    "--skip-log-bin".into(),
                ]);
                if let Some(certificates) = certificates {
                    command
                        .arg(format!(
                            "--ssl-cert={}",
                            certificates.join("server.pem").display()
                        ))
                        .arg(format!(
                            "--ssl-key={}",
                            certificates.join("server.key").display()
                        ))
                        .arg(format!(
                            "--ssl-ca={}",
                            certificates.join("server.pem").display()
                        ));
                }
                let child = spawn(&mut command, root);
                (child, "root", "fixture")
            }
            Engine::Sqlite => unreachable!(),
        };
        let connection = Connection::Network {
            engine,
            host: "127.0.0.1".into(),
            port,
            database: database.into(),
            username: username.into(),
            tls: Tls::Disable,
        };
        let mut server = Self {
            connection,
            child,
            _directory: directory,
        };
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            assert!(
                server.child.try_wait().unwrap().is_none(),
                "database server exited: {}",
                std::fs::read_to_string(server._directory.path().join("server.log"))
                    .unwrap_or_default()
            );
            if tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .is_ok()
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "database server startup timed out"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        if engine == Engine::Mysql {
            use sqlx::{Connection as _, Executor};
            let options = sqlx::mysql::MySqlConnectOptions::new()
                .host("127.0.0.1")
                .port(port)
                .username("root")
                .ssl_mode(sqlx::mysql::MySqlSslMode::Disabled);
            let mut connection = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
            connection
                .execute("CREATE DATABASE fixture CHARACTER SET utf8mb4")
                .await
                .unwrap();
            connection
                .execute(sqlx::AssertSqlSafe(format!(
                    "ALTER USER 'root'@'localhost' IDENTIFIED WITH caching_sha2_password BY '{PASSWORD}'"
                )))
                .await
                .unwrap();
            connection.execute("CREATE PROCEDURE fixture.results(IN empty_first BOOLEAN) BEGIN SELECT 11 AS `first_value` WHERE NOT empty_first; SELECT 22 AS `ignored_value`; END").await.unwrap();
            connection.close().await.unwrap();
        }
        server
    }
}
fn run(command: &mut Command) {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "database initialization failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn spawn(command: &mut Command, root: &Path) -> Child {
    let log = std::fs::File::create(root.join("server.log")).unwrap();
    command
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap()
}
