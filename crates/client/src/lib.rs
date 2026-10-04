pub mod activity;
pub mod commands;
pub mod conversation;
mod directory;
mod document;
mod files;
pub mod git;
pub mod host;
pub mod login;
pub mod mcp_login;
mod outcomes;
pub mod ports;
mod projection;
pub mod terminal;
mod transfers;
pub mod usage;
mod watch;
pub use document::{DocumentWrite, MAX_DOCUMENT_BYTES};
pub use files::FileView;
pub use transfers::FileMove;
pub use watch::View;

use std::sync::Arc;

use sailry_link::{Admission, Subscription, Transport};
use sailry_protocol::{Command, ErrorCode, Fault, NodeId, Output, Request, Update};

pub use projection::{Apply, Projection};

pub struct Client {
    transport: Arc<dyn Transport>,
    git: Option<Arc<git::Reads>>,
    usage_scope: Option<(sailry_protocol::plugin::Context, bool)>,
}

impl Client {
    /// Opening or reconnecting a stream never replays prior byte input.
    pub async fn open(
        &self,
        resource: sailry_protocol::StreamId,
    ) -> Result<sailry_link::Stream, Fault> {
        self.transport.open(resource).await
    }
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self {
            transport,
            git: None,
            usage_scope: None,
        }
    }
    /// Share Git reads between views belonging to the same controller.
    pub fn with_git_reads(mut self, reads: Arc<git::Reads>) -> Self {
        self.git = Some(reads);
        self
    }

    /// Scope report reads while retaining the core observer's Node event reduction.
    pub fn with_usage_scope(&self, context: sailry_protocol::plugin::Context) -> Self {
        Self {
            transport: self.transport.clone(),
            git: self.git.clone(),
            usage_scope: Some((context, false)),
        }
    }

    /// Resolve this exact package digest on another known Node before each read.
    /// Its own observer performs resolution, so an offline source cannot delay peers.
    pub fn with_matching_usage_scope(&self, context: sailry_protocol::plugin::Context) -> Self {
        Self {
            transport: self.transport.clone(),
            git: self.git.clone(),
            usage_scope: Some((context, true)),
        }
    }

    pub fn target(&self) -> NodeId {
        self.transport.target()
    }

    /// Retain this request when retrying after an uncertain transport result.
    pub fn prepare(&self, command: Command) -> Request {
        Request::new(self.target(), command)
    }

    pub async fn dispatch(&self, request: Request) -> Result<Admission, Fault> {
        if request.target != self.target() {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "request belongs to another Node",
            ));
        }
        self.transport.dispatch(request).await
    }

    pub async fn execute(&self, request: Request) -> Result<Output, Fault> {
        if request.target != self.target() {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "request belongs to another Node",
            ));
        }
        if let Some(reads) = &self.git {
            if git::is_read(&request.command) {
                return reads.execute(self.transport.clone(), request).await;
            }
            if request.command.durable() {
                reads.invalidate(request.target);
            }
        }
        let durable = request.command.durable();
        let result = execute(&*self.transport, request).await;
        if durable && let Some(reads) = &self.git {
            reads.invalidate(self.target());
        }
        result
    }

    pub async fn subscribe_browser(&self) -> Result<Box<dyn Subscription>, Fault> {
        self.transport
            .subscribe(sailry_protocol::Topic::Browser)
            .await
    }

    pub async fn subscribe(&self) -> Result<Box<dyn Subscription>, Fault> {
        self.transport.subscribe(sailry_protocol::Topic::Node).await
    }

    pub async fn recover(&self, projection: &mut Projection, generation: u64) -> Result<(), Fault> {
        match self.execute(self.prepare(Command::Snapshot)).await? {
            Output::Snapshot(snapshot) => {
                projection.apply(generation, Update::Snapshot(snapshot))?;
                Ok(())
            }
            _ => Err(Fault::new(
                ErrorCode::Internal,
                "snapshot response expected",
            )),
        }
    }
}

async fn execute(transport: &dyn Transport, request: Request) -> Result<Output, Fault> {
    let admission = transport.dispatch(request).await?;
    let output = admission.completion.await.map_err(|_| {
        Fault::new(
            ErrorCode::OutcomeUnknown,
            "completion unavailable; query using the same request identifier",
        )
    })??;
    if output.supported() {
        Ok(output)
    } else {
        Err(Fault::new(
            ErrorCode::Unavailable,
            "request result is not supported; the request will not be replayed",
        ))
    }
}
