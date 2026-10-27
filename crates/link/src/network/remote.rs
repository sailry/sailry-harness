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

mod diagnostics;
use diagnostics::CommandLog;

pub(super) struct Remote {
    pub(super) core: Arc<Core>,
    pub(super) address: EndpointAddr,
}

impl Transport for Remote {
    fn watch_connection(
        &self,
        updates: tokio::sync::watch::Sender<crate::ConnectionInfo>,
        stop: crate::CancellationToken,
    ) -> Pending<'_, ()> {
        Box::pin(super::connection::observe(
            &self.core,
            &self.address,
            updates,
            stop,
        ))
    }

    fn open(
        &self,
        resource: sailry_protocol::StreamId,
    ) -> Pending<'_, Result<crate::Stream, Fault>> {
        Box::pin(async move {
            let (connection, compressed) = self.core.connection(&self.address, true).await?;
            let (mut send, mut recv) = self
                .core
                .wait(connection.open_bi())
                .await?
                .map_err(network)?;
            self.core
                .call(&mut send, Call::OpenStream(resource), compressed)
                .await??;
            match self
                .core
                .wait(frame::read_with::<Reply>(&mut recv, compressed))
                .await??
            {
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
            let log = CommandLog::new(self.target(), request.id);
            if request.target != self.target() {
                let error = Fault::new(ErrorCode::WrongTarget, "request belongs to another Node");
                log.failed("target", "target_mismatch", &error);
                return Err(error);
            }
            log.begin();
            let id = request.id;
            let (connection, compressed) = match self.core.connection(&self.address, true).await {
                Ok(connection) => connection,
                Err(error) => {
                    log.failed(
                        "connect",
                        if self.core.stop.is_cancelled() {
                            "link_stopped"
                        } else {
                            "connect_failed"
                        },
                        &error,
                    );
                    return Err(error);
                }
            };
            let (mut send, mut recv) = match self.core.wait(connection.open_bi()).await {
                Ok(Ok(streams)) => streams,
                Ok(Err(error)) => {
                    let error = network(error);
                    log.failed("open_bi", "open_failed", &error);
                    return Err(error);
                }
                Err(error) => {
                    log.failed(
                        "open_bi",
                        if self.core.stop.is_cancelled() {
                            "link_stopped"
                        } else {
                            "open_timeout"
                        },
                        &error,
                    );
                    return Err(error);
                }
            };
            log.sending();
            match self
                .core
                .call(&mut send, Call::Command(Box::new(request)), compressed)
                .await
            {
                Ok(Ok(())) => {}
                Ok(Err(error)) if error.code == ErrorCode::InvalidRequest => {
                    log.failed("write", "encode_failed", &error);
                    return Err(error);
                }
                Ok(Err(_)) => {
                    let error = uncertain();
                    log.interrupted("write", "send_failed", &error, None);
                    return Err(error);
                }
                Err(_) => {
                    let error = uncertain();
                    log.interrupted(
                        "write",
                        if self.core.stop.is_cancelled() {
                            "link_stopped"
                        } else {
                            "send_timeout"
                        },
                        &error,
                        None,
                    );
                    return Err(error);
                }
            }
            if send.finish().is_err() {
                let error = uncertain();
                log.interrupted("finish", "finish_failed", &error, None);
                return Err(error);
            }
            // Once bytes may have reached the peer, a missing receipt does not
            // establish non-admission. The caller must retain the same request ID.
            let receipt = match self
                .core
                .wait(frame::read_with::<Reply>(&mut recv, compressed))
                .await
            {
                Ok(Ok(Reply::Receipt(receipt))) if receipt.id == id => receipt,
                Ok(Ok(Reply::Rejected(error))) => {
                    log.rejected(&error);
                    return Err(error);
                }
                Ok(Ok(Reply::Receipt(_))) => {
                    let error = uncertain();
                    log.interrupted("receipt", "receipt_mismatch", &error, None);
                    return Err(error);
                }
                Ok(Ok(_)) => {
                    let error = uncertain();
                    log.interrupted("receipt", "receipt_invalid", &error, None);
                    return Err(error);
                }
                Ok(Err(error)) => {
                    let reason = if error.code == ErrorCode::InvalidRequest {
                        "receipt_invalid"
                    } else {
                        "receipt_missing"
                    };
                    let error = uncertain();
                    log.interrupted("receipt", reason, &error, None);
                    return Err(error);
                }
                Err(_) => {
                    let error = uncertain();
                    log.interrupted(
                        "receipt",
                        if self.core.stop.is_cancelled() {
                            "link_stopped"
                        } else {
                            "receipt_timeout"
                        },
                        &error,
                        None,
                    );
                    return Err(error);
                }
            };
            log.admitted(receipt.durable);
            let (completion, result) = oneshot::channel();
            let core = self.core.clone();
            let observer_log = log.clone();
            if self
                .core
                .spawn(Box::pin(async move {
                    // Receipt confirms admission, not a business-operation deadline.
                    let response = tokio::select! {
                        biased;
                        _ = core.stop.cancelled() => {
                            let error = completion_unknown();
                            observer_log.interrupted(
                                "completion",
                                "link_stopped",
                                &error,
                                Some(receipt.durable),
                            );
                            Err(error)
                        },
                        reply = frame::read_reply(&mut recv, compressed) => match reply {
                            Ok(Reply::Completed(result)) => {
                                observer_log.completed(
                                    result.as_ref(),
                                    receipt.durable,
                                    super::connection::snapshot(&connection),
                                );
                                *result
                            },
                            Ok(Reply::Rejected(_)) => {
                                let error = completion_unknown();
                                observer_log.interrupted(
                                    "completion",
                                    "completion_rejected",
                                    &error,
                                    Some(receipt.durable),
                                );
                                Err(error)
                            },
                            Ok(_) => {
                                let error = completion_unknown();
                                observer_log.interrupted(
                                    "completion",
                                    "completion_invalid",
                                    &error,
                                    Some(receipt.durable),
                                );
                                Err(error)
                            }
                            Err(_) => {
                                let error = completion_unknown();
                                observer_log.interrupted(
                                    "completion",
                                    if core.stop.is_cancelled() {
                                        "link_stopped"
                                    } else {
                                        "completion_missing"
                                    },
                                    &error,
                                    Some(receipt.durable),
                                );
                                Err(error)
                            }
                        },
                    };
                    let _ = completion.send(response);
                }))
                .is_err()
            {
                let returned = Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "request admitted but completion observation could not start",
                );
                log.interrupted(
                    "completion_observer",
                    if self.core.stop.is_cancelled() {
                        "link_stopped"
                    } else {
                        "observer_unavailable"
                    },
                    &returned,
                    Some(receipt.durable),
                );
                return Err(returned);
            }
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
            let (connection, compressed) = self.core.connection(&self.address, true).await?;
            let (mut send, recv) = self
                .core
                .wait(connection.open_bi())
                .await?
                .map_err(network)?;
            self.core
                .call(&mut send, Call::Subscribe(topic), compressed)
                .await??;
            send.finish().map_err(network)?;
            let mut events = Events {
                connection,
                core: self.core.clone(),
                recv,
                reader: frame::Reader::new(compressed),
            };
            match events.reply().await? {
                Reply::Subscribed => Ok(Box::new(events) as Box<dyn Subscription>),
                Reply::Rejected(error) => Err(error),
                _ => Err(invalid("subscription acceptance expected")),
            }
        })
    }
}

fn uncertain() -> Fault {
    Fault::new(
        ErrorCode::OutcomeUnknown,
        "request may have been admitted; recover using the same request identifier",
    )
}

fn completion_unknown() -> Fault {
    Fault::new(
        ErrorCode::OutcomeUnknown,
        "completion unavailable; retry with the same request identifier",
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
            match self.reply().await? {
                Reply::Update(update) => Ok(*update),
                Reply::Rejected(error) => Err(error),
                _ => Err(invalid("subscription update expected")),
            }
        })
    }
}

impl Events {
    async fn reply(&mut self) -> Result<Reply, Fault> {
        // Subscription preparation and idle reads have no operation deadline.
        // Closing the Link or dropping this future still cancels observation.
        tokio::select! {
            biased;
            _ = self.core.stop.cancelled() => Err(network("closed")),
            reply = self.reader.reply(&mut self.recv) => reply.map_err(|error| {
                if matches!(self.connection.close_reason(), Some(ConnectionError::ApplicationClosed(close)) if close.error_code == 1u32.into()) {
                    Fault::new(ErrorCode::PermissionDenied, "peer revoked access")
                } else { error }
            }),
        }
    }
}

impl Drop for Events {
    fn drop(&mut self) {
        let _ = self.recv.stop(0u32.into());
    }
}
