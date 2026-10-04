use super::*;
use sailry_protocol::ssh;
use tokio::{net::TcpListener, task::JoinSet};

#[derive(Clone)]
pub(crate) struct Ssh {
    pub profile: ssh::Profile,
    pub credential: ssh::Credential,
}

pub(super) struct Tunnel {
    pub port: u16,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(super) async fn open(
    operation: &Operation,
    stop: &CancellationToken,
    closed: &CancellationToken,
) -> Result<Option<Tunnel>, Fault> {
    let Connection::Ssh { host, port, .. } = &operation.profile.connection else {
        return Ok(None);
    };
    let ssh = operation.ssh.as_ref().ok_or_else(unavailable)?;
    let session = match crate::ssh::connect(&ssh.profile, &ssh.credential, stop, closed).await? {
        crate::ssh::Connected::Session(session) => session,
        crate::ssh::Connected::HostKeyRequired { .. } => {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "Verify the SSH connection host key before connecting to the database",
            ));
        }
    };
    // The listener belongs to this operation, is loopback-only, and is dropped
    // with all channel tasks on completion, cancellation, or timeout.
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| unavailable())?;
    let local_port = listener.local_addr().map_err(|_| unavailable())?.port();
    let host = host.clone();
    let port = *port;
    let task = tokio::spawn(async move {
        let mut channels = JoinSet::new();
        loop {
            tokio::select! {
                incoming = listener.accept() => {
                    let Ok((mut stream, address)) = incoming else { break };
                    let channel = session.channel_open_direct_tcpip(
                        &host, port.into(), address.ip().to_string(), address.port().into(),
                    ).await;
                    let Ok(channel) = channel else { break };
                    channels.spawn(async move {
                        let _ = tokio::io::copy_bidirectional(&mut stream, &mut channel.into_stream()).await;
                    });
                }
                _ = channels.join_next(), if !channels.is_empty() => {}
            }
        }
    });
    Ok(Some(Tunnel {
        port: local_port,
        task,
    }))
}
