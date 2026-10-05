mod controller;
mod identity;
mod network;
pub mod rendezvous;
mod stream;

use std::{future::Future, pin::Pin, sync::Arc};

use sailry_protocol::{Fault, NodeId, Output, Receipt, Request, Topic, Update};
use tokio::sync::oneshot;

pub use identity::Identity;
pub use iroh::EndpointAddr;
pub use network::{
    Invitation, Link, LinkHandle, NetworkScope, PeerInfo, PeerStore, RelaySelection,
};
pub use stream::{ByteStream, Stream};
pub use tokio_util::sync::CancellationToken;

pub type Response = Result<Output, Fault>;
pub type Pending<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub struct Admission {
    pub receipt: Receipt,
    pub completion: oneshot::Receiver<Response>,
}

pub trait Subscription: Send {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>>;
}

/// Injected at the runtime boundary; Link never owns the business request ledger.
pub trait Handler: Send + Sync {
    /// Whether this authenticated endpoint owns a Node execution service.
    fn execution_available(&self) -> bool {
        true
    }

    /// The owning service must authorize the caller and resource before returning a stream.
    fn open(
        &self,
        _caller: NodeId,
        _resource: sailry_protocol::StreamId,
    ) -> Pending<'_, Result<Stream, Fault>> {
        Box::pin(async {
            Err(Fault::new(
                sailry_protocol::ErrorCode::NotFound,
                "stream resource is unavailable",
            ))
        })
    }
    fn dispatch(&self, caller: NodeId, request: Request) -> Pending<'_, Result<Admission, Fault>>;
    fn subscribe(
        &self,
        caller: NodeId,
        topic: Topic,
    ) -> Pending<'_, Result<Box<dyn Subscription>, Fault>>;
}

pub trait Transport: Send + Sync {
    fn open(&self, _resource: sailry_protocol::StreamId) -> Pending<'_, Result<Stream, Fault>> {
        Box::pin(async {
            Err(Fault::new(
                sailry_protocol::ErrorCode::NotFound,
                "stream resource is unavailable",
            ))
        })
    }
    fn target(&self) -> NodeId;
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>>;
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>>;
}

/// Constructed by the Node owner. The caller is bound once, never read from wire data.
pub struct Local {
    target: NodeId,
    caller: NodeId,
    handler: Arc<dyn Handler>,
}

impl Local {
    pub fn new(target: NodeId, caller: NodeId, handler: Arc<dyn Handler>) -> Self {
        Self {
            target,
            caller,
            handler,
        }
    }
}

impl Transport for Local {
    fn open(&self, resource: sailry_protocol::StreamId) -> Pending<'_, Result<Stream, Fault>> {
        self.handler.open(self.caller, resource)
    }
    fn target(&self) -> NodeId {
        self.target
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        self.handler.dispatch(self.caller, request)
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.handler.subscribe(self.caller, topic)
    }
}
