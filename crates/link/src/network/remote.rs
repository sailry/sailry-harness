use std::sync::Arc;

use iroh::{
    EndpointAddr,
    endpoint::{Connection, ConnectionError, RecvStream},
};
use sailry_protocol::{ErrorCode, Fault, NodeId, Request, Update};
use tokio::sync::oneshot;

use super::{
    Core,
    frame::{self, Call, Reply, invalid, network},
};
use crate::{Admission, Pending, Subscription, Transport};

pub(super) struct Remote {
    pub(super) core: Arc<Core>,
    pub(super) address: EndpointAddr,
}

impl Transport for Remote {
    fn open(
        &self,
        resource: sailry_protocol::StreamId,
    ) -> Pending<'_, Result<crate::Stream, Fault>> {
        Box::pin(async move {
            let connection = self.core.connection(&self.address).await?;
            let (mut send, mut recv) = self
                .core
                .wait(connection.open_bi())
                .await?
                .map_err(network)?;
            self.core
                .wait(frame::write(&mut send, &Call::OpenStream(resource)))
                .await??;
            match self.core.wait(frame::read::<Reply>(&mut recv)).await?? {
                Reply::StreamOpened => {
                    Ok(Box::new(super::stream::NetworkStream::new(send, recv)) as crate::Stream)
                }
                Reply::Rejected(error) => Err(error),
                _ => Err(invalid("stream acknowledgement expected")),
            }
        })
    }
    fn target(&self) -> NodeId {
        NodeId(*self.address.id.as_bytes())
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            if request.target != self.target() {
                return Err(Fault::new(
                    ErrorCode::WrongTarget,
                    "request belongs to another Node",
                ));
            }
            let id = request.id;
            let connection = self.core.connection(&self.address).await?;
            let (mut send, mut recv) = self
                .core
                .wait(connection.open_bi())
                .await?
                .map_err(network)?;
            self.core
                .wait(frame::write(&mut send, &Call::Command(Box::new(request))))
                .await
                .map_err(|_| uncertain())?
                .map_err(|error| {
                    if error.code == ErrorCode::InvalidRequest {
                        error
                    } else {
                        uncertain()
                    }
                })?;
            send.finish().map_err(|_| uncertain())?;
            // Once bytes may have reached the peer, a missing receipt does not
            // establish non-admission. The caller must retain the same request ID.
            let receipt = match self.core.wait(frame::read::<Reply>(&mut recv)).await {
                Ok(Ok(Reply::Receipt(receipt))) if receipt.id == id => receipt,
                Ok(Ok(Reply::Rejected(error))) => return Err(error),
                _ => return Err(uncertain()),
            };
            let (completion, result) = oneshot::channel();
            let core = self.core.clone();
            self.core
                .spawn(Box::pin(async move {
                    let response = match core.wait(frame::read::<Reply>(&mut recv)).await {
                        Ok(Ok(Reply::Completed(result))) => *result,
                        _ => Err(Fault::new(
                            ErrorCode::OutcomeUnknown,
                            "completion unavailable; retry with the same request identifier",
                        )),
                    };
                    let _ = completion.send(response);
                }))
                .map_err(|_| {
                    Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "request admitted but completion observation could not start",
                    )
                })?;
            Ok(Admission {
                receipt,
                completion: result,
            })
        })
    }

    fn subscribe(
        &self,
        topic: sailry_protocol::Topic,
    ) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async move {
            let call = match topic {
                sailry_protocol::Topic::Commands(session) => Call::WatchCommands(session),
                sailry_protocol::Topic::Browser => Call::WatchBrowser,
                sailry_protocol::Topic::Node => Call::Subscribe,
                sailry_protocol::Topic::ProviderLogin(id) => Call::WatchProviderLogin(id),
                sailry_protocol::Topic::McpLogin(id) => Call::WatchMcpLogin(id),
                sailry_protocol::Topic::Files(worktree) => Call::WatchFiles(worktree),
                sailry_protocol::Topic::Terminal(terminal) => Call::WatchTerminal(terminal),
                sailry_protocol::Topic::Conversation(session) => Call::WatchConversation(session),
            };
            let connection = self.core.connection(&self.address).await?;
            let (mut send, recv) = self
                .core
                .wait(connection.open_bi())
                .await?
                .map_err(network)?;
            self.core.wait(frame::write(&mut send, &call)).await??;
            send.finish().map_err(network)?;
            Ok(Box::new(Events {
                connection,
                core: self.core.clone(),
                recv,
                reader: frame::Reader::default(),
            }) as Box<dyn Subscription>)
        })
    }
}

fn uncertain() -> Fault {
    Fault::new(
        ErrorCode::OutcomeUnknown,
        "request may have been admitted; recover using the same request identifier",
    )
}

struct Events {
    connection: Connection,
    core: Arc<Core>,
    recv: RecvStream,
    reader: frame::Reader,
}

impl Subscription for Events {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            // Idle subscriptions have no read timeout; shutdown and stream reset still interrupt.
            let reply: Reply = tokio::select! {
                biased;
                _ = self.core.stop.cancelled() => return Err(network("closed")),
                reply = self.reader.read(&mut self.recv) => reply.map_err(|error| {
                    if matches!(self.connection.close_reason(), Some(ConnectionError::ApplicationClosed(close)) if close.error_code == 1u32.into()) {
                        Fault::new(ErrorCode::PermissionDenied, "peer revoked access")
                    } else { error }
                })?,
            };
            match reply {
                Reply::Update(update) => Ok(*update),
                Reply::Rejected(error) => Err(error),
                _ => Err(invalid("subscription update expected")),
            }
        })
    }
}

impl Drop for Events {
    fn drop(&mut self) {
        let _ = self.recv.stop(0u32.into());
    }
}
