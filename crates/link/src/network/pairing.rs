use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use iroh::{EndpointAddr, SecretKey, endpoint::Connection};
use sailry_protocol::{ErrorCode, Fault, NodeId};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use super::{
    Core, LinkHandle,
    frame::{self, network},
};
use crate::CancellationToken;

pub(super) const ALPN: &[u8] = b"sailry/pair/1";
pub const LIFETIME: Duration = Duration::from_secs(60);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ticket {
    version: u8,
    address: EndpointAddr,
    token: [u8; 32],
    expires_at_ms: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    token: [u8; 32],
    address: EndpointAddr,
}

pub(super) struct Offer {
    token: [u8; 32],
    deadline: Instant,
    stop: CancellationToken,
    peer: Option<NodeId>,
    paired: watch::Sender<bool>,
}

/// Owned by a pairing view. Dropping it stops accepting its unpublished or live ticket.
/// Neither the ticket nor its bearer secret implements Debug.
pub struct Invitation {
    ticket: String,
    expires_at_ms: u64,
    stop: CancellationToken,
    paired: watch::Receiver<bool>,
    deadline: Instant,
    closed: CancellationToken,
}

impl Invitation {
    pub fn ticket(&self) -> &str {
        &self.ticket
    }
    pub fn expires_at_ms(&self) -> u64 {
        self.expires_at_ms
    }
    pub fn is_paired(&self) -> bool {
        *self.paired.borrow()
    }
    pub async fn paired(&mut self) -> Result<(), Fault> {
        while !*self.paired.borrow() {
            tokio::select! {
                biased;
                _ = self.stop.cancelled() => return Err(rejected()),
                _ = self.closed.cancelled() => return Err(network("closed")),
                _ = tokio::time::sleep_until(self.deadline.into()) => return Err(rejected()),
                changed = self.paired.changed() => { changed.map_err(network)?; },
            }
        }
        Ok(())
    }
}

impl Drop for Invitation {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl LinkHandle {
    /// Replaces the previous open invitation, without changing established peer trust.
    pub fn invite(&self) -> Result<Invitation, Fault> {
        if self.core.stop.is_cancelled() {
            return Err(network("closed"));
        }
        let token = SecretKey::generate().to_bytes();
        let expires_at_ms = now_ms()? + LIFETIME.as_millis() as u64;
        let ticket = serde_json::to_string(&Ticket {
            version: 1,
            address: self.address(),
            token,
            expires_at_ms,
        })
        .map_err(network)?;
        let stop = CancellationToken::new();
        let deadline = Instant::now() + LIFETIME;
        let (paired, receiver) = watch::channel(false);
        let mut offer = self.core.invitation.lock().map_err(network)?;
        if let Some(previous) = offer.take() {
            previous.stop.cancel();
        }
        *offer = Some(Offer {
            token,
            deadline,
            stop: stop.clone(),
            peer: None,
            paired,
        });
        Ok(Invitation {
            ticket,
            expires_at_ms,
            stop,
            paired: receiver,
            deadline,
            closed: self.core.stop.clone(),
        })
    }

    /// Iroh verifies the endpoint key in the ticket. The execution Node must then
    /// accept its short-lived invitation before either side grants normal access.
    pub async fn pair(&self, ticket: &str) -> Result<EndpointAddr, Fault> {
        if ticket.len() > 8192 {
            return Err(rejected());
        }
        let ticket: Ticket = serde_json::from_str(ticket).map_err(|_| rejected())?;
        let now = now_ms()?;
        if ticket.version != 1
            || ticket.expires_at_ms <= now
            || ticket.expires_at_ms > now + LIFETIME.as_millis() as u64
            || ticket.address.id == self.address().id
            || ticket.address.addrs.len() > 16
        {
            return Err(rejected());
        }
        let connection = self
            .core
            .wait(self.core.endpoint.connect(ticket.address.clone(), ALPN))
            .await?
            .map_err(network)?;
        let result = async {
            let (mut send, mut recv) = self
                .core
                .wait(connection.open_bi())
                .await?
                .map_err(network)?;
            self.core
                .wait(frame::write(
                    &mut send,
                    &Claim {
                        token: ticket.token,
                        address: self.address(),
                    },
                ))
                .await??;
            send.finish().map_err(network)?;
            let accepted: Result<(), Fault> = self.core.wait(frame::read(&mut recv)).await??;
            accepted?;
            self.set_trust(NodeId(*connection.remote_id().as_bytes()), true)
                .await?;
            self.remember(ticket.address.clone()).await
        }
        .await;
        connection.close(0u32.into(), b"pairing finished");
        result?;
        Ok(ticket.address)
    }
}

pub(super) async fn serve(core: Arc<Core>, connection: Connection, peer: NodeId) {
    let result = async {
        let (mut send, mut recv) = core.wait(connection.accept_bi()).await?.map_err(network)?;
        let claim: Claim = core.wait(frame::read_limited(&mut recv, 8192)).await??;
        let accepted =
            if NodeId(*claim.address.id.as_bytes()) != peer || claim.address.addrs.len() > 16 {
                Err(rejected())
            } else {
                accept(&core, claim.token, peer, claim.address).await
            };
        core.wait(frame::write(&mut send, &accepted)).await??;
        send.finish().map_err(network)?;
        let _ = core.wait(send.stopped()).await;
        Ok::<_, Fault>(())
    }
    .await;
    let _ = result;
    connection.close(0u32.into(), b"pairing finished");
}

async fn accept(
    core: &Arc<Core>,
    token: [u8; 32],
    peer: NodeId,
    address: EndpointAddr,
) -> Result<(), Fault> {
    {
        let mut current = core.invitation.lock().map_err(network)?;
        let offer = current.as_mut().ok_or_else(rejected)?;
        // One atomic first claimant; later retries must carry the same certified identity.
        if offer.token != token
            || offer.stop.is_cancelled()
            || Instant::now() >= offer.deadline
            || offer.peer.is_some_and(|owner| owner != peer)
            || (*offer.paired.borrow() && !core.trusted(peer))
        {
            return Err(rejected());
        }
        offer.peer = Some(peer);
    }
    LinkHandle { core: core.clone() }
        .set_trust(peer, true)
        .await?;
    LinkHandle { core: core.clone() }.remember(address).await?;
    let current = core.invitation.lock().map_err(network)?;
    if let Some(offer) = current.as_ref().filter(|offer| offer.token == token) {
        offer.paired.send_replace(true);
    }
    Ok(())
}

fn now_ms() -> Result<u64, Fault> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(network)?
            .as_millis(),
    )
    .map_err(network)
}
fn rejected() -> Fault {
    Fault::new(
        ErrorCode::PermissionDenied,
        "pairing invitation is unavailable",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticket_preserves_addresses() {
        let address = EndpointAddr::new(SecretKey::generate().public())
            .with_relay_url("https://relay-one.example".parse().unwrap())
            .with_relay_url("https://relay-two.example".parse().unwrap())
            .with_ip_addr("192.0.2.1:1234".parse().unwrap());
        let ticket = Ticket {
            version: 1,
            address: address.clone(),
            token: [1; 32],
            expires_at_ms: 60_000,
        };
        let decoded: Ticket =
            serde_json::from_str(&serde_json::to_string(&ticket).unwrap()).unwrap();
        assert_eq!(decoded.address, address);
        assert_eq!(decoded.address.relay_urls().count(), 2);
    }
}
