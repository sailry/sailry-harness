use super::*;
use russh::{
    Channel,
    keys::{HashAlg, PrivateKey, ssh_key::private::Ed25519Keypair},
    server::{self, Server as _},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub struct Server {
    port: u16,
    key: ssh::HostKey,
    authentication: Arc<AtomicUsize>,
    forwarded: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Server {
    pub async fn start(connection: &Connection) -> Self {
        let Connection::Network { port: target, .. } = connection else {
            unreachable!()
        };
        let key: PrivateKey = Ed25519Keypair::from_seed(&[47; 32]).into();
        let observed = ssh::HostKey {
            algorithm: key.public_key().algorithm().to_string(),
            fingerprint: key.public_key().fingerprint(HashAlg::Sha256).to_string(),
        };
        let config = Arc::new(server::Config {
            keys: vec![key],
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..Default::default()
        });
        let authentication = Arc::new(AtomicUsize::new(0));
        let forwarded = Arc::new(AtomicUsize::new(0));
        let mut handler = Handler {
            target: *target,
            authentication: authentication.clone(),
            forwarded: forwarded.clone(),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            handler.run_on_socket(config, &listener).await.unwrap();
        });
        Self {
            port,
            key: observed,
            authentication,
            forwarded,
            task,
        }
    }

    pub fn forwarded(&self) -> usize {
        self.forwarded.load(Ordering::SeqCst)
    }

    pub async fn save(&self, client: &Client) -> SshId {
        let profile = ssh::Profile {
            sharing: None,
            id: SshId::new(),
            revision: 0,
            name: "Database tunnel".into(),
            host: "127.0.0.1".into(),
            port: self.port,
            username: "fixture".into(),
            authentication: ssh::Authentication::Password,
            host_key: None,
        };
        let Output::SshProfile(profile) = execute(
            client,
            Command::SaveSsh {
                profile,
                expected_revision: 0,
                credential: Some(ssh::Credential::Password {
                    password: Secret::new("tunnel-password".into()),
                }),
            },
        )
        .await
        else {
            panic!("SSH profile expected")
        };
        let database = Profile {
            sharing: None,
            id: DatabaseId::new(),
            revision: 0,
            name: "Untrusted tunnel".into(),
            read_only: true,
            connection: Connection::Ssh {
                engine: Engine::Mysql,
                host: "127.0.0.1".into(),
                port: 1,
                database: String::new(),
                username: "fixture".into(),
                tls: database::Tls::Disable,
                ssh: profile.id,
            },
        };
        let before = self.authentication.load(Ordering::SeqCst);
        let error = client
            .execute(client.prepare(Command::TestDatabase {
                profile: database,
                expected_revision: 0,
                password: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        assert!(error.message.contains("host key"));
        assert_eq!(self.authentication.load(Ordering::SeqCst), before);
        execute(
            client,
            Command::TrustSsh {
                profile: profile.id,
                expected_revision: profile.revision,
                key: self.key.clone(),
            },
        )
        .await;
        profile.id
    }
}

#[derive(Clone)]
struct Handler {
    target: u16,
    authentication: Arc<AtomicUsize>,
    forwarded: Arc<AtomicUsize>,
}
impl server::Server for Handler {
    type Handler = Self;
    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self {
        self.clone()
    }
    fn handle_session_error(&mut self, _: russh::Error) {}
}
impl server::Handler for Handler {
    type Error = russh::Error;
    async fn auth_password(
        &mut self,
        username: &str,
        password: &str,
    ) -> Result<server::Auth, Self::Error> {
        self.authentication.fetch_add(1, Ordering::SeqCst);
        Ok(if username == "fixture" && password == "tunnel-password" {
            server::Auth::Accept
        } else {
            server::Auth::reject()
        })
    }
    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<server::Msg>,
        host: &str,
        port: u32,
        _: &str,
        _: u32,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, u32::from(self.target));
        let mut tcp = tokio::net::TcpStream::connect((host, self.target))
            .await
            .unwrap();
        self.forwarded.fetch_add(1, Ordering::SeqCst);
        reply.accept().await;
        tokio::spawn(async move {
            let _ = tokio::io::copy_bidirectional(&mut tcp, &mut channel.into_stream()).await;
        });
        Ok(())
    }
}
