//! Real-engine checks for the SQLx transport options used by the Node adapter.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sqlx::{ConnectOptions as _, Connection as _, Executor, Row};

const HOST: &str = "database.sailry.test";

pub struct Certificates {
    pub directory: tempfile::TempDir,
    pem: Vec<u8>,
}
impl Certificates {
    pub fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec![HOST.into()]).unwrap();
        let pem = encode("CERTIFICATE", cert.der());
        std::fs::write(directory.path().join("server.pem"), &pem).unwrap();
        let key = directory.path().join("server.key");
        std::fs::write(&key, encode("PRIVATE KEY", &signing_key.serialize_der())).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(key, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        Self { directory, pem }
    }
}
fn encode(kind: &str, bytes: &[u8]) -> Vec<u8> {
    format!(
        "-----BEGIN {kind}-----\n{}\n-----END {kind}-----\n",
        STANDARD.encode(bytes)
    )
    .into_bytes()
}

pub async fn verify(server: &servers::Server, certificates: &Certificates) {
    let options = server.connection.server().unwrap();
    for preferred in [false, true] {
        match server.connection.engine() {
            Engine::Postgres => {
                use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
                let options = PgConnectOptions::new_without_pgpass()
                    .host(HOST)
                    .connect_addr(std::net::Ipv4Addr::LOCALHOST.into())
                    .port(options.port)
                    .username(options.username)
                    .database(options.database)
                    .password(servers::PASSWORD)
                    .ssl_mode(if preferred {
                        PgSslMode::Prefer
                    } else {
                        PgSslMode::VerifyFull
                    })
                    .ssl_verify_server(true)
                    .ssl_root_cert_from_pem(certificates.pem.clone())
                    .disable_statement_logging();
                let mut connection = PgConnection::connect_with(&options).await.unwrap();
                assert!(
                    connection
                        .fetch_one("SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()")
                        .await
                        .unwrap()
                        .try_get::<bool, _>(0)
                        .unwrap()
                );
                connection.close().await.unwrap();
                let error = PgConnection::connect_with(&options.host("wrong.sailry.test"))
                    .await
                    .unwrap_err();
                assert!(matches!(error, sqlx::Error::Tls(_)), "{error}");
            }
            Engine::Mysql => {
                use sqlx::mysql::{MySqlConnectOptions, MySqlConnection, MySqlSslMode};
                let options = MySqlConnectOptions::new()
                    .host(HOST)
                    .connect_addr(std::net::Ipv4Addr::LOCALHOST.into())
                    .port(options.port)
                    .username(options.username)
                    .database(options.database)
                    .password(servers::PASSWORD)
                    .ssl_mode(if preferred {
                        MySqlSslMode::Preferred
                    } else {
                        MySqlSslMode::VerifyIdentity
                    })
                    .ssl_verify_server(true)
                    .ssl_ca_from_pem(certificates.pem.clone())
                    .disable_statement_logging();
                let mut connection = MySqlConnection::connect_with(&options).await.unwrap();
                let cipher = connection
                    .fetch_one("SHOW SESSION STATUS LIKE 'Ssl_cipher'")
                    .await
                    .unwrap()
                    .try_get::<String, _>(1)
                    .unwrap();
                assert!(!cipher.is_empty());
                connection.close().await.unwrap();
                let error = MySqlConnection::connect_with(&options.host("wrong.sailry.test"))
                    .await
                    .unwrap_err();
                assert!(matches!(error, sqlx::Error::Tls(_)), "{error}");
            }
            Engine::Sqlite => unreachable!(),
        }
    }
}
