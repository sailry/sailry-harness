//! Isolated SSH peer. Normal commands execute in its temporary directory.
use russh::{
    Channel, ChannelId,
    keys::{HashAlg, PrivateKey, PublicKey, ssh_key::private::Ed25519Keypair},
    server::{self, Server as _},
};
use sailry_link::CancellationToken;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[path = "server/process.rs"]
mod process;
#[cfg(unix)]
#[path = "server/terminal.rs"]
mod terminal;

pub fn key(seed: u8) -> PrivateKey {
    Ed25519Keypair::from_seed(&[seed; 32]).into()
}

pub fn sftp_executable() -> &'static str {
    let paths = [
        #[cfg(windows)]
        r"C:\Windows\System32\OpenSSH\sftp-server.exe",
        #[cfg(unix)]
        "/usr/libexec/sftp-server",
        #[cfg(unix)]
        "/usr/lib/openssh/sftp-server",
        #[cfg(unix)]
        "/usr/lib/ssh/sftp-server",
    ];
    paths
        .into_iter()
        .find(|path| std::path::Path::new(path).is_file())
        .unwrap_or_else(|| {
            panic!(
                "OpenSSH SFTP server prerequisite is missing; expected {}",
                paths.join(" or ")
            )
        })
}

pub struct Server {
    pub port: u16,
    pub key: sailry_protocol::ssh::HostKey,
    pub authentication: Arc<AtomicUsize>,
    pub commands: Arc<Mutex<Vec<String>>>,
    pub subsystems: Arc<AtomicUsize>,
    stop: CancellationToken,
    task: tokio::task::JoinHandle<()>,
    processes: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}

impl Server {
    pub async fn start(root: PathBuf, seed: u8) -> Self {
        Self::configured(
            root,
            seed,
            #[cfg(unix)]
            false,
            None,
        )
        .await
    }
    #[cfg(unix)]
    pub async fn deployment(root: PathBuf, seed: u8) -> Self {
        Self::configured(root, seed, true, None).await
    }
    pub async fn without_close(root: PathBuf, seed: u8) -> Self {
        Self::configured(
            root,
            seed,
            #[cfg(unix)]
            false,
            Some("close"),
        )
        .await
    }
    async fn configured(
        root: PathBuf,
        seed: u8,
        #[cfg(unix)] deployment: bool,
        denied_requests: Option<&'static str>,
    ) -> Self {
        let key = key(seed);
        let observed = sailry_protocol::ssh::HostKey {
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
        let commands = Arc::new(Mutex::new(Vec::new()));
        let subsystems = Arc::new(AtomicUsize::new(0));
        let processes = Arc::new(Mutex::new(Vec::new()));
        let mut handler = Handler {
            root,
            #[cfg(unix)]
            deployment,
            denied_requests,
            authentication: authentication.clone(),
            commands: commands.clone(),
            subsystems: subsystems.clone(),
            channels: BTreeMap::new(),
            #[cfg(unix)]
            terminals: BTreeMap::new(),
            processes: processes.clone(),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = CancellationToken::new();
        let stopping = stop.clone();
        let task = tokio::spawn(async move {
            let running = handler.run_on_socket(config, &listener);
            let handle = running.handle();
            tokio::pin!(running);
            tokio::select! {
                result = &mut running => result.unwrap(),
                _ = stopping.cancelled() => {
                    handle.shutdown("fixture closed".into());
                    running.await.unwrap();
                }
            }
        });
        Self {
            port,
            key: observed,
            authentication,
            commands,
            subsystems,
            stop,
            task,
            processes,
        }
    }

    pub async fn close(self) {
        self.stop.cancel();
        tokio::time::timeout(Duration::from_secs(5), self.task)
            .await
            .unwrap()
            .unwrap();
        let tasks = std::mem::take(&mut *self.processes.lock().unwrap());
        for task in tasks {
            task.await.unwrap();
        }
    }
}

struct Handler {
    root: PathBuf,
    #[cfg(unix)]
    deployment: bool,
    denied_requests: Option<&'static str>,
    authentication: Arc<AtomicUsize>,
    commands: Arc<Mutex<Vec<String>>>,
    subsystems: Arc<AtomicUsize>,
    channels: BTreeMap<ChannelId, Channel<server::Msg>>,
    #[cfg(unix)]
    terminals: BTreeMap<ChannelId, terminal::Pty>,
    processes: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}
impl Clone for Handler {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
            #[cfg(unix)]
            deployment: self.deployment,
            denied_requests: self.denied_requests,
            authentication: self.authentication.clone(),
            commands: self.commands.clone(),
            subsystems: self.subsystems.clone(),
            channels: BTreeMap::new(),
            #[cfg(unix)]
            terminals: BTreeMap::new(),
            processes: self.processes.clone(),
        }
    }
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
        user: &str,
        password: &str,
    ) -> Result<server::Auth, Self::Error> {
        self.authentication.fetch_add(1, Ordering::SeqCst);
        Ok(
            if user == "fixture" && password == "isolated-ssh-password" {
                server::Auth::Accept
            } else {
                server::Auth::reject()
            },
        )
    }
    async fn auth_publickey(
        &mut self,
        user: &str,
        public: &PublicKey,
    ) -> Result<server::Auth, Self::Error> {
        self.authentication.fetch_add(1, Ordering::SeqCst);
        Ok(if user == "fixture" && public == key(43).public_key() {
            server::Auth::Accept
        } else {
            server::Auth::reject()
        })
    }
    async fn channel_open_session(
        &mut self,
        channel: Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        self.channels.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }
    #[cfg(unix)]
    async fn pty_request(
        &mut self,
        channel: ChannelId,
        term: &str,
        columns: u32,
        rows: u32,
        width: u32,
        height: u32,
        _: &[(russh::Pty, u32)],
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        assert_eq!(term, "xterm-256color");
        let pty = terminal::Pty::new(columns, rows, width, height);
        self.terminals.insert(channel, pty);
        session.channel_success(channel)?;
        Ok(())
    }
    #[cfg(unix)]
    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        self.commands
            .lock()
            .unwrap()
            .push("interactive-shell".into());
        let pty = self.terminals.get_mut(&channel).unwrap();
        let pipe = self.channels.remove(&channel).unwrap();
        self.processes
            .lock()
            .unwrap()
            .push(pty.start(self.root.clone(), pipe, session.handle()));
        session.channel_success(channel)?;
        Ok(())
    }
    #[cfg(unix)]
    async fn window_change_request(
        &mut self,
        channel: ChannelId,
        columns: u32,
        rows: u32,
        width: u32,
        height: u32,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if let Some(pty) = self.terminals.get(&channel) {
            pty.resize(columns, rows, width, height);
        }
        Ok(())
    }
    #[cfg(unix)]
    async fn channel_close(
        &mut self,
        channel: ChannelId,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        self.terminals.remove(&channel);
        Ok(())
    }
    async fn subsystem_request(
        &mut self,
        channel: ChannelId,
        name: &str,
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if name != "sftp" {
            session.channel_failure(channel)?;
            return Ok(());
        }
        let mut command = tokio::process::Command::new(sftp_executable());
        command.args(["-d", "."]);
        if let Some(requests) = self.denied_requests {
            command.args(["-P", requests]);
        }
        self.subsystems.fetch_add(1, Ordering::SeqCst);
        session.channel_success(channel)?;
        let pipe = self.channels.remove(&channel).unwrap();
        self.processes
            .lock()
            .unwrap()
            .push(tokio::spawn(process::run(
                self.root.clone(),
                command,
                pipe,
                session.handle(),
            )));
        Ok(())
    }
    #[cfg(unix)]
    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        let command = String::from_utf8(data.to_vec()).unwrap();
        let command = if self.deployment {
            command
                .replace("$HOME", self.root.to_str().unwrap())
                .replace(
                    "ai.sailry.host",
                    &format!(
                        "ai.sailry.host.acceptance.{}",
                        self.root.file_name().unwrap().to_str().unwrap()
                    ),
                )
        } else {
            command
        };
        self.commands.lock().unwrap().push(command.clone());
        session.channel_success(channel)?;
        let pipe = self.channels.remove(&channel).unwrap();
        if command.starts_with("scp ") || self.deployment {
            let mut process = tokio::process::Command::new("sh");
            process.arg("-c").arg(command).env("HOME", &self.root).env(
                "PATH",
                format!(
                    "{}:{}",
                    self.root.join(".fixture/bin").display(),
                    std::env::var("PATH").unwrap()
                ),
            );
            self.processes
                .lock()
                .unwrap()
                .push(tokio::spawn(process::run(
                    self.root.clone(),
                    process,
                    pipe,
                    session.handle(),
                )));
            return Ok(());
        }
        match command.as_str() {
            "hold" => return Ok(()),
            "unknown" => {
                session.close(channel)?;
                return Ok(());
            }
            "large" => {
                for _ in 0..16 {
                    session.data(channel, vec![b'x'; 16384])?;
                }
                session.exit_status_request(channel, 0)?;
            }
            _ => {
                let output = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg(&command)
                    .current_dir(&self.root)
                    .kill_on_drop(true)
                    .output()
                    .await
                    .unwrap();
                session.data(channel, output.stdout)?;
                session.extended_data(channel, 1, output.stderr)?;
                session.exit_status_request(channel, output.status.code().unwrap() as u32)?;
            }
        }
        session.eof(channel)?;
        session.close(channel)?;
        Ok(())
    }

    #[cfg(windows)]
    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        self.commands
            .lock()
            .unwrap()
            .push(String::from_utf8(data.to_vec()).unwrap());
        session.channel_failure(channel)?;
        Ok(())
    }

    #[cfg(windows)]
    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        self.commands
            .lock()
            .unwrap()
            .push("interactive-shell".into());
        session.channel_failure(channel)?;
        Ok(())
    }
}
