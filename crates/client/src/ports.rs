//! Controller loopback listeners use the same authenticated Client as other tools.
use crate::{Apply, Client, Projection};
use sailry_link::{CancellationToken, Subscription, Transport};
use sailry_protocol::{Command, ErrorCode, Fault, NodeId, Output};
use std::{net::Ipv4Addr, sync::Arc};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::watch,
    task::{JoinHandle, JoinSet},
};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", content = "error", rename_all = "snake_case")]
pub enum State {
    Listening,
    Closed,
    Failed(Fault),
}

mod services;
pub use services::Source;

pub struct Forwarder {
    pub node: NodeId,
    pub remote_port: u16,
    pub local_port: u16,
    pub state: watch::Receiver<State>,
    stop: CancellationToken,
    task: Option<JoinHandle<()>>,
    service_watch: Option<JoinHandle<()>>,
}
impl Forwarder {
    pub async fn close(mut self) {
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
        if let Some(task) = self.service_watch.take() {
            let _ = task.await;
        }
    }
}
impl Drop for Forwarder {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
impl Client {
    pub async fn forward_port(
        &self,
        remote_port: u16,
        local_port: u16,
    ) -> Result<Forwarder, Fault> {
        let mut subscription = self.subscribe().await?;
        let mut projection = Projection::new(self.target(), 1);
        loop {
            let applied = projection.apply(1, subscription.next().await?)?;
            if applied == Apply::Recover {
                self.recover(&mut projection, 1).await?;
            }
            if projection.snapshot().is_some() {
                break;
            }
        }
        // Establish the route once before exposing a local listener. Each TCP client
        // then gets its own single-use stream; prior bytes are never replayed.
        drop(self.port_stream(remote_port).await?);
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, local_port))
            .await
            .map_err(io_error)?;
        let local_port = listener.local_addr().map_err(io_error)?.port();
        let stop = CancellationToken::new();
        let cancellation = stop.clone();
        let client = Client::new(self.transport.clone());
        let (updates, state) = watch::channel(State::Listening);
        let task = tokio::spawn(async move {
            let finished = cancellation.clone();
            let result = run(
                client,
                listener,
                subscription,
                projection,
                remote_port,
                cancellation,
            )
            .await;
            finished.cancel();
            updates.send_replace(match result {
                Ok(()) => State::Closed,
                Err(error) => State::Failed(error),
            });
        });
        Ok(Forwarder {
            node: self.target(),
            remote_port,
            local_port,
            state,
            stop,
            task: Some(task),
            service_watch: None,
        })
    }
    async fn port_stream(&self, port: u16) -> Result<sailry_link::Stream, Fault> {
        let Output::PortStream { stream } = self
            .execute(self.prepare(Command::OpenPort { port }))
            .await?
        else {
            return Err(Fault::new(ErrorCode::Internal, "port stream expected"));
        };
        match self.open(stream).await {
            Ok(stream) => Ok(stream),
            Err(error) => {
                let _ = self
                    .execute(self.prepare(Command::CancelPort { stream }))
                    .await;
                Err(error)
            }
        }
    }
}
async fn run(
    client: Client,
    listener: TcpListener,
    mut subscription: Box<dyn Subscription>,
    mut projection: Projection,
    port: u16,
    stop: CancellationToken,
) -> Result<(), Fault> {
    let mut connections = JoinSet::new();
    let outcome = loop {
        tokio::select! {
            biased;
            _ = stop.cancelled() => break Ok(()),
            update = subscription.next() => {
                let applied = update.and_then(|update| projection.apply(1, update));
                match applied {
                    Err(error) => break Err(error),
                    Ok(Apply::Recover) => if let Err(error) = client.recover(&mut projection, 1).await { break Err(error); },
                    _ => {},
                }
            },
            result = connections.join_next(), if !connections.is_empty() => {
                if let Some(Err(_)) = result { break Err(Fault::new(ErrorCode::Internal, "port forwarding task failed")); }
            },
            accepted = listener.accept(), if connections.len() < 16 => {
                let (tcp, _) = match accepted { Ok(accepted) => accepted, Err(error) => break Err(io_error(error)) };
                connections.spawn(copy(client.transport.clone(), tcp, port));
            },
        }
    };
    drop(listener);
    connections.shutdown().await;
    outcome
}
async fn copy(transport: Arc<dyn Transport>, mut tcp: TcpStream, port: u16) -> Result<(), Fault> {
    let client = Client::new(transport);
    let mut stream = client.port_stream(port).await?;
    tokio::io::copy_bidirectional(&mut tcp, &mut stream)
        .await
        .map_err(io_error)?;
    Ok(())
}
fn io_error(error: std::io::Error) -> Fault {
    Fault::new(
        ErrorCode::Unavailable,
        format!("port forwarding failed: {error}"),
    )
}
