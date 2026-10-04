use std::sync::Arc;

use iroh::endpoint::{Connection, RecvStream, SendStream};
use sailry_protocol::{ErrorCode, Fault, NodeId};
use tokio::task::JoinSet;

use super::{
    Core, MAX_STREAMS,
    frame::{self, Call, Reply, network},
};

pub(super) async fn serve(core: Arc<Core>, connection: Connection, peer: NodeId) {
    let mut streams = JoinSet::new();
    // Reserve capacity for command and event streams while bulk transfers are active.
    let auxiliary = Arc::new(tokio::sync::Semaphore::new(4));
    // Conversation tabs retain subscriptions. Leave room for bulk streams and commands.
    let subscriptions = Arc::new(tokio::sync::Semaphore::new(MAX_STREAMS - 8));
    let mut trust_changed = core.trust_changed.subscribe();
    loop {
        tokio::select! {
            biased;
            _ = core.stop.cancelled() => break,
            _ = trust_changed.changed() => {
                if !core.trusted(peer) { break; }
            },
            _ = streams.join_next(), if !streams.is_empty() => {},
            stream = connection.accept_bi(), if streams.len() < MAX_STREAMS => {
                let Ok((send, recv)) = stream else { break; };
                let core = core.clone();
                let auxiliary = auxiliary.clone();
                let subscriptions = subscriptions.clone();
                streams.spawn(async move { let _ = request(core, peer, send, recv, auxiliary, subscriptions).await; });
            }
        }
    }
    streams.shutdown().await;
    if core.trusted(peer) {
        connection.close(0u32.into(), b"Link closed");
    } else {
        connection.close(1u32.into(), b"peer revoked");
    }
}

async fn request(
    core: Arc<Core>,
    peer: NodeId,
    mut send: SendStream,
    mut recv: RecvStream,
    auxiliary: Arc<tokio::sync::Semaphore>,
    subscriptions: Arc<tokio::sync::Semaphore>,
) -> Result<(), Fault> {
    let call: Call = core.wait(frame::read(&mut recv)).await??;
    if !core.trusted(peer) {
        core.wait(frame::write(
            &mut send,
            &Reply::Rejected(Fault::new(
                ErrorCode::PermissionDenied,
                "peer is no longer paired",
            )),
        ))
        .await??;
        return send.finish().map_err(network);
    }
    match call {
        Call::Inspect => {
            let name = core.name.read().map_err(network)?.clone();
            core.wait(frame::write(
                &mut send,
                &Reply::Endpoint {
                    execution: core.handler.execution_available(),
                    name,
                    platform: std::env::consts::OS.to_owned(),
                },
            ))
            .await??;
        }
        Call::OpenStream(resource) => {
            let capacity = auxiliary
                .try_acquire_owned()
                .ok()
                .zip(core.auxiliary.clone().try_acquire_owned().ok());
            let Some(_permits) = capacity else {
                core.wait(frame::write(
                    &mut send,
                    &Reply::Rejected(Fault::new(ErrorCode::Busy, "stream capacity exhausted")),
                ))
                .await??;
                return send.finish().map_err(network);
            };
            let mut service = match core.wait(core.handler.open(peer, resource)).await? {
                Ok(stream) => stream,
                Err(error) => {
                    core.wait(frame::write(&mut send, &Reply::Rejected(error)))
                        .await??;
                    return send.finish().map_err(network);
                }
            };
            core.wait(frame::write(&mut send, &Reply::StreamOpened))
                .await??;
            let mut stream = super::stream::NetworkStream::new(send, recv);
            // Tokio maintains two bounded 8 KiB buffers and propagates half-close.
            tokio::io::copy_bidirectional(&mut stream, &mut service)
                .await
                .map_err(network)?;
            return Ok(());
        }
        Call::Command(request) => {
            let admission = if request.command.durable() {
                core.handler.dispatch(peer, *request).await
            } else {
                tokio::select! {
                    biased;
                    _ = core.stop.cancelled() => return Ok(()),
                    _ = send.stopped() => return Ok(()),
                    result = core.handler.dispatch(peer, *request) => result,
                }
            };
            match admission {
                Ok(admission) => {
                    core.wait(frame::write(&mut send, &Reply::Receipt(admission.receipt)))
                        .await??;
                    let output = admission.completion.await.unwrap_or_else(|_| {
                        Err(Fault::new(
                            ErrorCode::OutcomeUnknown,
                            "Node completion is unavailable",
                        ))
                    });
                    core.wait(frame::write(&mut send, &Reply::Completed(Box::new(output))))
                        .await??;
                }
                Err(error) => {
                    core.wait(frame::write(&mut send, &Reply::Rejected(error)))
                        .await??;
                }
            }
        }
        subscription @ (Call::Subscribe
        | Call::WatchProviderLogin(_)
        | Call::WatchMcpLogin(_)
        | Call::WatchFiles(_)
        | Call::WatchTerminal(_)
        | Call::WatchBrowser
        | Call::WatchCommands(_)
        | Call::WatchConversation(_)) => {
            let Ok(_permit) = subscriptions.try_acquire_owned() else {
                core.wait(frame::write(
                    &mut send,
                    &Reply::Rejected(Fault::new(
                        ErrorCode::Busy,
                        "subscription capacity exhausted",
                    )),
                ))
                .await??;
                return send.finish().map_err(network);
            };
            let topic = match subscription {
                Call::WatchProviderLogin(id) => sailry_protocol::Topic::ProviderLogin(id),
                Call::WatchMcpLogin(id) => sailry_protocol::Topic::McpLogin(id),
                Call::WatchFiles(worktree) => sailry_protocol::Topic::Files(worktree),
                Call::WatchTerminal(terminal) => sailry_protocol::Topic::Terminal(terminal),
                Call::WatchCommands(session) => sailry_protocol::Topic::Commands(session),
                Call::WatchBrowser => sailry_protocol::Topic::Browser,
                Call::WatchConversation(session) => sailry_protocol::Topic::Conversation(session),
                _ => sailry_protocol::Topic::Node,
            };
            // Preparing a transient subscription has no durable admission. A
            // departed observer must release its slot even if setup is slow.
            let prepared = tokio::select! {
                biased;
                _ = core.stop.cancelled() => return Ok(()),
                _ = send.stopped() => return Ok(()),
                result = core.handler.subscribe(peer, topic) => result,
            };
            let mut subscription = match prepared {
                Ok(subscription) => subscription,
                Err(error) => {
                    core.wait(frame::write(&mut send, &Reply::Rejected(error)))
                        .await??;
                    return send.finish().map_err(network);
                }
            };
            loop {
                let update = tokio::select! {
                    biased;
                    _ = core.stop.cancelled() => break,
                    _ = send.stopped() => break,
                    update = subscription.next() => update?,
                };
                if !core.trusted(peer) {
                    break;
                }
                core.wait(frame::write(&mut send, &Reply::Update(Box::new(update))))
                    .await??;
            }
        }
    }
    send.finish().map_err(network)
}
