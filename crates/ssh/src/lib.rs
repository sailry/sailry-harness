//! Verified SSH connections shared by remote commands, terminals and database tunnels.
//! Reuses host-key verification semantics from
//! sailry-code 67ae9fa0 rust/crates/sailry-connections/src/ssh.rs.
use russh::{
    client,
    keys::{HashAlg, PublicKey},
};
use sailry_protocol::{
    ErrorCode, Fault, Secret,
    ssh::{Credential, HostKey, Profile},
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

mod authentication;
mod transport;

pub struct Verifier {
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

pub enum Connected {
    Session(client::Handle<Verifier>),
    HostKeyRequired { key: HostKey, changed: bool },
}

pub async fn connect(
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
    let connecting = async {
        let config = Arc::new(client::Config::default());
        let socket = transport::open(&profile.host, profile.port)
            .await
            .map_err(russh::Error::from)?;
        if config.nodelay
            && let Err(error) = socket.set_nodelay(true)
        {
            tracing::warn!("SSH TCP_NODELAY failed: {error}");
        }
        client::connect_stream(config, socket, handler).await
    };
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
        Err(error) => return Err(unavailable(&format!("SSH connection failed: {error}"))),
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

fn unavailable(message: &str) -> Fault {
    Fault::new(ErrorCode::Unavailable, message)
}
fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "SSH connection cancelled")
}
