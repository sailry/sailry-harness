use iroh::endpoint::{RecvStream, SendStream};
use sailry_protocol::{ErrorCode, Fault, MAX_FRAME_BYTES, Receipt, Request, Update};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::Response;

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
    WatchCommands(sailry_protocol::SessionId),
    WatchConversation(sailry_protocol::SessionId),
    WatchProviderLogin(sailry_protocol::RequestId),
    WatchMcpLogin(sailry_protocol::RequestId),
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub(super) enum Reply {
    Endpoint {
        execution: bool,
        name: Option<String>,
        platform: String,
    },
    StreamOpened,
    Receipt(Receipt),
    Rejected(Fault),
    Completed(Box<Response>),
    Update(Box<Update>),
}

pub(super) async fn write(stream: &mut SendStream, value: &impl Serialize) -> Result<(), Fault> {
    let bytes = zeroize::Zeroizing::new(serde_json::to_vec(value).map_err(invalid)?);
    length(bytes.len())?;
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .await
        .map_err(network)?;
    stream.write_all(&bytes).await.map_err(network)
}

pub(super) async fn read<T: DeserializeOwned>(stream: &mut RecvStream) -> Result<T, Fault> {
    read_limited(stream, MAX_FRAME_BYTES).await
}

pub(super) async fn read_limited<T: DeserializeOwned>(
    stream: &mut RecvStream,
    maximum: usize,
) -> Result<T, Fault> {
    let mut prefix = [0; 4];
    stream.read_exact(&mut prefix).await.map_err(network)?;
    let size = u32::from_be_bytes(prefix) as usize;
    // Verify untrusted length before allocating payload memory.
    length(size)?;
    if size > maximum {
        return Err(invalid("frame exceeds protocol limit"));
    }
    let mut bytes = zeroize::Zeroizing::new(vec![0; size]);
    stream.read_exact(&mut bytes).await.map_err(network)?;
    serde_json::from_slice(&bytes).map_err(invalid)
}

/// Retains partial prefix/payload across cancellation of a subscription's next() future.
#[derive(Default)]
pub(super) struct Reader {
    prefix: [u8; 4],
    prefix_read: usize,
    payload: Vec<u8>,
    payload_read: usize,
}

impl Reader {
    pub(super) async fn read<T: DeserializeOwned>(
        &mut self,
        stream: &mut RecvStream,
    ) -> Result<T, Fault> {
        while self.prefix_read < self.prefix.len() {
            let count = stream
                .read(&mut self.prefix[self.prefix_read..])
                .await
                .map_err(network)?
                .ok_or_else(|| network("unexpected EOF"))?;
            self.prefix_read += count;
        }
        let size = u32::from_be_bytes(self.prefix) as usize;
        length(size)?;
        self.payload.resize(size, 0);
        while self.payload_read < size {
            let count = stream
                .read(&mut self.payload[self.payload_read..])
                .await
                .map_err(network)?
                .ok_or_else(|| network("unexpected EOF"))?;
            self.payload_read += count;
        }
        let value = serde_json::from_slice(&self.payload).map_err(invalid);
        self.prefix_read = 0;
        self.payload_read = 0;
        self.payload.clear();
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
    Fault::new(ErrorCode::InvalidRequest, "invalid Link frame")
}

#[cfg(test)]
mod tests {
    use super::*;

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
