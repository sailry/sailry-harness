//! Loopback forwarding follows sailry-code 67ae9fa0 host-runtime tunnels.rs.
//! Link supplies authentication and byte transport; no separate tunnel protocol.
use sailry_link::{CancellationToken, Stream};
use sailry_protocol::{ErrorCode, Fault, NodeId, StreamId};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{net::TcpStream, sync::Notify};

mod stream;

#[derive(Default)]
pub(crate) struct Ports {
    entries: Mutex<BTreeMap<StreamId, Entry>>,
    stopped: CancellationToken,
    changed: Notify,
}
struct Entry {
    caller: NodeId,
    port: u16,
    expires: Instant,
    active: bool,
    stop: CancellationToken,
}
struct Lease {
    ports: Arc<Ports>,
    id: StreamId,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.ports.entries.lock().unwrap().remove(&self.id);
        self.ports.changed.notify_one();
    }
}
impl Ports {
    pub(crate) fn prepare(&self, caller: NodeId, port: u16) -> Result<StreamId, Fault> {
        if port == 0 {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "port must be nonzero",
            ));
        }
        let mut entries = self.entries.lock().unwrap();
        if self.stopped.is_cancelled() {
            return Err(unavailable());
        }
        entries.retain(|_, entry| entry.active || entry.expires > Instant::now());
        if entries.len() >= 32 {
            return Err(Fault::new(
                ErrorCode::Busy,
                "port stream capacity exhausted",
            ));
        }
        let id = StreamId::new();
        entries.insert(
            id,
            Entry {
                caller,
                port,
                expires: Instant::now() + Duration::from_secs(30),
                active: false,
                stop: self.stopped.child_token(),
            },
        );
        Ok(id)
    }
    pub(crate) async fn open(
        self: &Arc<Self>,
        caller: NodeId,
        id: StreamId,
    ) -> Result<Option<Stream>, Fault> {
        let (port, stop) = {
            let mut entries = self.entries.lock().unwrap();
            let Some(entry) = entries.get_mut(&id) else {
                return Ok(None);
            };
            if entry.caller != caller {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "port stream belongs to another controller",
                ));
            }
            if entry.active || entry.expires <= Instant::now() || entry.stop.is_cancelled() {
                return Err(unavailable());
            }
            entry.active = true;
            (entry.port, entry.stop.clone())
        };
        let lease = Lease {
            ports: self.clone(),
            id,
        };
        let mut tcp = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(unavailable()),
            result = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))) => result.map_err(|_| unavailable())?.map_err(|_| unavailable())?,
        };
        let (client, mut service) = tokio::io::duplex(16 * 1024);
        let client_stop = stop.clone();
        tokio::spawn(async move {
            let _lease = lease;
            tokio::select! {
                biased;
                _ = stop.cancelled() => {},
                _ = tokio::io::copy_bidirectional(&mut tcp, &mut service) => {},
            }
        });
        Ok(Some(Box::new(stream::PortStream::new(client, client_stop))))
    }
    pub(crate) fn cancel(&self, caller: NodeId, id: StreamId) -> Result<(), Fault> {
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.get(&id) {
            if entry.caller != caller {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "port stream belongs to another controller",
                ));
            }
            entry.stop.cancel();
            if !entry.active {
                entries.remove(&id);
            }
        }
        Ok(())
    }
    pub(crate) fn stop(&self) {
        let mut entries = self.entries.lock().unwrap();
        self.stopped.cancel();
        entries.retain(|_, entry| entry.active);
    }
    pub(crate) async fn join(&self) {
        loop {
            let changed = self.changed.notified();
            if self.entries.lock().unwrap().is_empty() {
                return;
            }
            changed.await;
        }
    }
}
fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "execution port is unavailable")
}
