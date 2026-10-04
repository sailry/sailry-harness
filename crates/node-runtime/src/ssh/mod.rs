//! Reuses host-key verification and execution semantics from
//! sailry-code 67ae9fa0 rust/crates/sailry-connections/src/ssh.rs.
use russh::{
    ChannelMsg, Disconnect, client,
    keys::{HashAlg, PublicKey},
};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault, Secret,
    ssh::{Credential, HostKey, Outcome, Profile},
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

mod authentication;
pub(crate) mod files;
mod install;
pub(crate) mod transfer;

const OUTPUT_LIMIT: usize = 128 * 1024;

pub(crate) struct Operation {
    pub profile: Profile,
    pub credential: Credential,
    pub task: Task,
}

pub(crate) enum Task {
    Files(files::Operation),
    Check,
    Install,
    Terminal(sailry_protocol::ssh::TerminalLaunch),
    Run {
        command: String,
        timeout_ms: u64,
    },
    Transfer {
        root: std::path::PathBuf,
        profile: Option<std::path::PathBuf>,
        transfer: sailry_protocol::ssh::Transfer,
        timeout_ms: u64,
    },
}

pub(crate) struct Verifier {
    trusted: Option<HostKey>,
    observed: Arc<Mutex<Option<HostKey>>>,
}

impl client::Handler for Verifier {
    type Error = russh::Error;
    async fn check_server_key(&mut self, key: &PublicKey) -> Result<bool, Self::Error> {
        let key = HostKey {
            algorithm: key.algorithm().to_string(),
            fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
        };
        let accepted = self.trusted.as_ref() == Some(&key);
        *self
            .observed
            .lock()
            .expect("SSH host key observer poisoned") = Some(key);
        Ok(accepted)
    }
}

pub(crate) enum Connected {
    Session(client::Handle<Verifier>),
    HostKeyRequired { key: HostKey, changed: bool },
}

pub(crate) async fn connect(
    profile: &Profile,
    credential: &Credential,
    stop: &CancellationToken,
    closed: &CancellationToken,
) -> Result<Connected, Fault> {
    let observed = Arc::new(Mutex::new(None));
    let handler = Verifier {
        trusted: profile.host_key.clone(),
        observed: observed.clone(),
    };
    let connecting = client::connect(
        Arc::new(client::Config::default()),
        (profile.host.as_str(), profile.port),
        handler,
    );
    let connected = tokio::select! {
        _ = stop.cancelled() => return Err(cancelled()),
        _ = closed.cancelled() => return Err(cancelled()),
        result = tokio::time::timeout(Duration::from_secs(30), connecting) => result.map_err(|_| unavailable("SSH connection timed out"))?,
    };
    let key = observed
        .lock()
        .expect("SSH host key observer poisoned")
        .clone();
    let mut session = match connected {
        Ok(session) => session,
        Err(_)
            if key
                .as_ref()
                .is_some_and(|key| Some(key) != profile.host_key.as_ref()) =>
        {
            return Ok(Connected::HostKeyRequired {
                key: key.expect("observed key"),
                changed: profile.host_key.is_some(),
            });
        }
        Err(_) => return Err(unavailable("SSH connection failed")),
    };
    let authenticated = tokio::select! {
        _ = stop.cancelled() => Err(cancelled()),
        _ = closed.cancelled() => Err(cancelled()),
        result = tokio::time::timeout(Duration::from_secs(30), authentication::authenticate(&mut session, &profile.username, credential)) =>
            result.unwrap_or_else(|_| Err(unavailable("SSH authentication timed out"))),
    };
    match authenticated {
        Ok(true) => Ok(Connected::Session(session)),
        Ok(false) => Err(Fault::new(
            ErrorCode::PermissionDenied,
            "SSH authentication failed",
        )),
        Err(error) => Err(error),
    }
}

pub(crate) async fn execute(
    operation: Operation,
    stop: &CancellationToken,
    closed: &CancellationToken,
    terminals: Arc<crate::terminal::Terminals>,
    caller: sailry_protocol::NodeId,
    deployment: Option<(
        sailry_link::LinkHandle,
        tokio::sync::watch::Sender<sailry_protocol::ssh::InstallProgress>,
    )>,
    transfers: Arc<crate::files::transfers::Transfers>,
) -> Result<Outcome, Fault> {
    let session = match connect(&operation.profile, &operation.credential, stop, closed).await? {
        Connected::Session(session) => session,
        Connected::HostKeyRequired { key, changed } => {
            return Ok(Outcome::HostKeyRequired { key, changed });
        }
    };
    let result = match operation.task {
        Task::Terminal(launch) => {
            let opening = crate::terminal::SshConnection::open(session, &launch.viewport);
            let connection = tokio::select! {
                _ = stop.cancelled() => return Err(unknown("SSH terminal opening cancelled; inspect terminals before retrying")),
                _ = closed.cancelled() => return Err(unknown("SSH terminal opening interrupted; inspect terminals before retrying")),
                result = tokio::time::timeout(Duration::from_secs(30), opening) => result.map_err(|_| unknown("SSH terminal opening timed out; inspect the remote process before retrying"))??,
            };
            let source = crate::terminal::Source::Ssh {
                profile: operation.profile.id,
                connection,
            };
            return tokio::task::spawn_blocking(move || {
                terminals.create_ssh(caller, &launch, source)
            })
            .await
            .map_err(|_| unknown("SSH terminal registration outcome is unavailable"))?
            .map_err(|error| {
                unknown(&format!(
                    "SSH shell opened but terminal registration failed: {}",
                    error.message
                ))
            })
            .map(Outcome::Terminal);
        }
        Task::Run {
            command,
            timeout_ms,
        } => {
            tokio::select! {
                _ = stop.cancelled() => Err(unknown("SSH command cancelled; inspect the remote process before retrying")),
                _ = closed.cancelled() => Err(unknown("SSH connection closed; inspect the remote process before retrying")),
                result = tokio::time::timeout(Duration::from_millis(timeout_ms), run(&session, &command)) =>
                    result.unwrap_or_else(|_| Err(unknown("SSH command timed out; inspect the remote process before retrying"))),
            }
        }
        Task::Files(task) => {
            tokio::select! {
                _ = stop.cancelled() => Err(unknown("SSH file operation cancelled; inspect the remote target before retrying")),
                _ = closed.cancelled() => Err(unknown("SSH file operation interrupted; inspect the remote target before retrying")),
                result = tokio::time::timeout(Duration::from_secs(300), files::execute(&session, &operation.profile, task, caller, transfers, closed.clone())) =>
                    result.unwrap_or_else(|_| Err(unknown("SSH file operation timed out; inspect the remote target before retrying"))),
            }
        }
        Task::Check => Ok(Outcome::Connected),
        Task::Install => {
            let (link, progress) =
                deployment.ok_or_else(|| unavailable("Node Link is unavailable"))?;
            tokio::select! {
                _ = stop.cancelled() => Err(unknown("Host installation cancelled; inspect the host before retrying")),
                _ = closed.cancelled() => Err(unknown("Host installation interrupted; inspect the host before retrying")),
                result = tokio::time::timeout(Duration::from_secs(600), install::execute(&session, &link, &progress)) => result.unwrap_or_else(|_| Err(unknown("Host installation timed out; inspect the host before retrying"))),
            }
        }
        Task::Transfer {
            root,
            profile,
            transfer,
            timeout_ms,
        } => transfer::execute(&session, root, profile, transfer, timeout_ms, stop, closed).await,
    };
    let _ = tokio::time::timeout(
        Duration::from_secs(1),
        session.disconnect(Disconnect::ByApplication, "", "en"),
    )
    .await;
    result
}

async fn run(session: &client::Handle<Verifier>, command: &str) -> Result<Outcome, Fault> {
    let mut channel = session
        .channel_open_session()
        .await
        .map_err(|_| unavailable("SSH channel could not be opened"))?;
    channel
        .exec(true, command)
        .await
        .map_err(|_| unknown("SSH command receipt is unavailable"))?;
    channel
        .eof()
        .await
        .map_err(|_| unknown("SSH command connection was interrupted"))?;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut exit_code = None;
    let mut truncated = false;
    while let Some(message) = channel.wait().await {
        match message {
            ChannelMsg::Data { data } => append(&mut stdout, &data, &mut truncated),
            ChannelMsg::ExtendedData { data, .. } => append(&mut stderr, &data, &mut truncated),
            ChannelMsg::ExitStatus { exit_status } => exit_code = Some(exit_status),
            ChannelMsg::Failure => {
                return Err(Fault::new(
                    ErrorCode::Unavailable,
                    "SSH server rejected the command",
                ));
            }
            _ => {}
        }
    }
    let exit_code = exit_code.ok_or_else(|| unknown("SSH command ended without an exit status"))?;
    Ok(Outcome::Completed {
        exit_code,
        stdout: String::from_utf8_lossy(&stdout).into(),
        stderr: String::from_utf8_lossy(&stderr).into(),
        truncated,
    })
}

fn append(output: &mut Vec<u8>, bytes: &[u8], truncated: &mut bool) {
    let count = bytes.len().min(OUTPUT_LIMIT - output.len());
    output.extend_from_slice(&bytes[..count]);
    *truncated |= count < bytes.len();
}
fn unavailable(message: &str) -> Fault {
    Fault::new(ErrorCode::Unavailable, message)
}
fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "SSH connection cancelled")
}
fn unknown(message: &str) -> Fault {
    Fault::new(ErrorCode::OutcomeUnknown, message)
}
