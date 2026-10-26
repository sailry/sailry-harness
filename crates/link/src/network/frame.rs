use iroh::endpoint::{RecvStream, SendStream};
use sailry_protocol::{ErrorCode, Fault, MAX_FRAME_BYTES, Receipt, Request, Update};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::time::Instant;
use zeroize::Zeroize;

use super::diagnostics;
use crate::Response;

mod snapshots;

#[derive(Serialize, Deserialize)]
pub(super) struct Envelope {
    #[serde(default)]
    pub software_version: Option<String>,
    #[serde(flatten)]
    pub call: Call,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub(super) enum Call {
    Inspect,
    OpenStream(sailry_protocol::StreamId),
    Command(Box<Request>),
    Subscribe,
    WatchFiles(sailry_protocol::WorktreeId),
    WatchTerminal(sailry_protocol::TerminalId),
    WatchBrowser,
    WatchPython,
    WatchInstallations,
    WatchScopedInstallations(Box<Request>),
    WatchCommands(sailry_protocol::SessionId),
    WatchConversation(sailry_protocol::SessionId),
    WatchProviderLogin(sailry_protocol::RequestId),
    WatchConnectorLogin(sailry_protocol::RequestId),
    WatchConnections,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub(super) enum Reply {
    Endpoint {
        #[serde(default)]
        software_version: Option<String>,
        execution: bool,
        name: Option<String>,
        platform: String,
    },
    StreamOpened,
    Receipt(Receipt),
    Rejected(Fault),
    Completed(Box<Response>),
    Update(Box<Update>),
    SnapshotStart(snapshots::Header),
    SnapshotPage(snapshots::Page),
}

/// Snapshot paging preserves one complete service result and its captured cursor.
pub(super) async fn write_reply(
    core: &super::Core,
    stream: &mut SendStream,
    reply: &Reply,
) -> Result<(), Fault> {
    snapshots::write(core, stream, reply).await
}

pub(super) async fn read_reply(stream: &mut RecvStream) -> Result<Reply, Fault> {
    Reader::default().reply(stream).await
}

pub(super) async fn write(stream: &mut SendStream, value: &impl Serialize) -> Result<(), Fault> {
    let started = Instant::now();
    let bytes = zeroize::Zeroizing::new(serde_json::to_vec(value).map_err(invalid)?);
    length(bytes.len())?;
    let limit = 4 + bytes.len();
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .await
        .map_err(|error| diagnostics::write(error, "write_prefix", started, 0, limit))?;
    stream
        .write_all(&bytes)
        .await
        .map_err(|error| diagnostics::write(error, "write_payload", started, 4, limit))
}

pub(super) async fn read<T: DeserializeOwned>(stream: &mut RecvStream) -> Result<T, Fault> {
    read_limited(stream, MAX_FRAME_BYTES).await
}

pub(super) async fn read_limited<T: DeserializeOwned>(
    stream: &mut RecvStream,
    maximum: usize,
) -> Result<T, Fault> {
    let started = Instant::now();
    let mut prefix = [0; 4];
    stream
        .read_exact(&mut prefix)
        .await
        .map_err(|error| diagnostics::exact(error, "read_prefix", started, 0, 4))?;
    let size = u32::from_be_bytes(prefix) as usize;
    // Verify untrusted length before allocating payload memory.
    length(size)?;
    if size > maximum {
        return Err(invalid("frame exceeds protocol limit"));
    }
    let mut bytes = zeroize::Zeroizing::new(vec![0; size]);
    stream
        .read_exact(&mut bytes)
        .await
        .map_err(|error| diagnostics::exact(error, "read_payload", started, 4, 4 + size))?;
    serde_json::from_slice(&bytes).map_err(invalid)
}

/// Retains partial prefix/payload across cancellation of a subscription's next() future.
#[derive(Default)]
pub(super) struct Reader {
    prefix: [u8; 4],
    prefix_read: usize,
    payload: zeroize::Zeroizing<Vec<u8>>,
    payload_read: usize,
    snapshot: snapshots::Assembly,
}

impl Reader {
    /// Keep page assembly here so cancelling next() cannot expose a partial snapshot.
    pub(super) async fn reply(&mut self, stream: &mut RecvStream) -> Result<Reply, Fault> {
        loop {
            let reply = self.read(stream).await?;
            if let Some(reply) = self.snapshot.accept(reply)? {
                return Ok(reply);
            }
        }
    }

    pub(super) async fn read<T: DeserializeOwned>(
        &mut self,
        stream: &mut RecvStream,
    ) -> Result<T, Fault> {
        let started = Instant::now();
        while self.prefix_read < self.prefix.len() {
            let count = stream
                .read(&mut self.prefix[self.prefix_read..])
                .await
                .map_err(|error| {
                    diagnostics::read(error, "read_prefix", started, self.prefix_read, 4)
                })?
                .ok_or_else(|| diagnostics::eof("read_prefix", started, self.prefix_read, 4))?;
            self.prefix_read += count;
        }
        let size = u32::from_be_bytes(self.prefix) as usize;
        length(size)?;
        self.payload.resize(size, 0);
        while self.payload_read < size {
            let count = stream
                .read(&mut self.payload[self.payload_read..])
                .await
                .map_err(|error| {
                    diagnostics::read(
                        error,
                        "read_payload",
                        started,
                        4 + self.payload_read,
                        4 + size,
                    )
                })?
                .ok_or_else(|| {
                    diagnostics::eof("read_payload", started, 4 + self.payload_read, 4 + size)
                })?;
            self.payload_read += count;
        }
        let value = serde_json::from_slice(&self.payload).map_err(invalid);
        self.prefix_read = 0;
        self.payload_read = 0;
        self.payload.zeroize();
        value
    }
}

fn length(size: usize) -> Result<(), Fault> {
    if size == 0 || size > MAX_FRAME_BYTES {
        Err(invalid("invalid frame size"))
    } else {
        Ok(())
    }
}

pub(super) fn network(_: impl std::fmt::Display) -> Fault {
    Fault::new(ErrorCode::Unavailable, "Link connection is unavailable")
}
pub(super) fn invalid(_: impl std::fmt::Display) -> Fault {
    Fault::invalid("invalid Link frame")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnoses_missing_software_version() {
        let envelope: Envelope =
            serde_json::from_value(serde_json::json!({"kind":"inspect"})).unwrap();
        assert!(envelope.software_version.is_none());
        assert!(matches!(envelope.call, Call::Inspect));
        let wire = serde_json::to_value(Envelope {
            software_version: Some(crate::PRODUCT_VERSION.into()),
            call: Call::Inspect,
        })
        .unwrap();
        assert_eq!(wire["kind"], "inspect");
        assert_eq!(wire["software_version"], crate::PRODUCT_VERSION);
        let reply: Reply = serde_json::from_value(serde_json::json!({"kind":"endpoint", "data":{
            "execution":true,"name":null,"platform":"linux"
        }}))
        .unwrap();
        assert!(matches!(
            reply,
            Reply::Endpoint {
                software_version: None,
                ..
            }
        ));
    }

    #[test]
    fn retains_completion_wire_shape() {
        let result: Response = Err(Fault::new(ErrorCode::PermissionDenied, "fixture denied"));
        let wire = serde_json::json!({"kind":"completed", "data":result});
        let reply: Reply = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&reply).unwrap(), wire);
        let Reply::Completed(actual) = reply else {
            panic!("completion expected")
        };
        assert_eq!(*actual, result);
    }

    #[test]
    fn retains_update_wire_shape() {
        let update = Update::FilesChanged {
            node: sailry_protocol::NodeId([1; 32]),
            worktree: sailry_protocol::WorktreeId::new(),
        };
        let wire = serde_json::json!({"kind":"update", "data":update});
        let reply: Reply = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&reply).unwrap(), wire);
        let Reply::Update(actual) = reply else {
            panic!("update expected")
        };
        assert_eq!(*actual, update);
    }
    #[test]
    fn rejects_oversized_prefix() {
        assert!(length(MAX_FRAME_BYTES + 1).is_err());
        assert!(length(0).is_err());
        assert!(length(MAX_FRAME_BYTES).is_ok());
    }

    #[tokio::test]
    async fn cancellation_preserves_prefix() {
        use iroh::{Endpoint, RelayMode, endpoint::presets};
        use std::time::Duration;
        let server = Endpoint::builder(presets::Minimal)
            .relay_mode(RelayMode::Disabled)
            .clear_ip_transports()
            .bind_addr((std::net::Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .alpns(vec![b"frame-test".to_vec()])
            .bind()
            .await
            .unwrap();
        let client = Endpoint::builder(presets::Minimal)
            .relay_mode(RelayMode::Disabled)
            .clear_ip_transports()
            .bind_addr((std::net::Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .bind()
            .await
            .unwrap();
        let (outgoing, incoming) =
            tokio::join!(client.connect(server.addr(), b"frame-test"), async {
                server.accept().await.unwrap().await.unwrap()
            });
        let outgoing = outgoing.unwrap();
        let (mut send, _) = outgoing.open_bi().await.unwrap();
        let payload = serde_json::to_vec(&"payload").unwrap();
        let prefix = (payload.len() as u32).to_be_bytes();
        send.write_all(&prefix[..2]).await.unwrap();
        let (_, mut recv) = incoming.accept_bi().await.unwrap();
        let mut reader = Reader::default();
        assert!(
            tokio::time::timeout(Duration::from_millis(30), reader.read::<String>(&mut recv))
                .await
                .is_err()
        );
        assert_eq!(reader.prefix_read, 2);
        send.write_all(&prefix[2..]).await.unwrap();
        send.write_all(&payload).await.unwrap();
        send.finish().unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), reader.read::<String>(&mut recv))
                .await
                .unwrap()
                .unwrap(),
            "payload"
        );
        client.close().await;
        server.close().await;
    }
}
