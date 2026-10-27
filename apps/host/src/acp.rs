//! ACP translates editor requests into the existing Node's Client commands and projections.
mod connection;
mod controls;
mod observe;
mod prompt;
mod sessions;
#[cfg(test)]
mod tests;

use adk_acp::agent_client_protocol as sdk;
use sailry_client::{
    Client,
    conversation::{HistoryRequest, View},
};
use sailry_link::CancellationToken;
use sailry_protocol::{Command, ErrorCode, Fault, Output, Request, RequestId, Session, SessionId};
use sdk::{ConnectionTo, schema::v1 as acp};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::PathBuf,
    sync::{Arc, atomic::AtomicU64},
};
use tokio::{
    sync::{Mutex, Semaphore, mpsc, watch},
    task::JoinHandle,
};

pub(super) struct Adapter {
    client: Arc<Client>,
    sessions: Mutex<BTreeMap<SessionId, Arc<Opened>>>,
    stop: CancellationToken,
    details: Semaphore,
}

struct Opened {
    session: Mutex<Session>,
    view: watch::Receiver<View>,
    requests: mpsc::Sender<HistoryRequest>,
    stop: CancellationToken,
    observer: JoinHandle<std::result::Result<(), Fault>>,
    presentation: Mutex<observe::Presentation>,
    history: AtomicU64,
    problem: watch::Sender<Option<Problem>>,
}

#[derive(Clone)]
struct Problem {
    error: Fault,
    operation: Value,
}

impl Drop for Opened {
    fn drop(&mut self) {
        self.stop.cancel();
        self.observer.abort();
    }
}

impl Drop for Adapter {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Adapter {
    pub(super) fn new(client: Arc<Client>) -> Arc<Self> {
        Arc::new(Self {
            client,
            sessions: Mutex::new(BTreeMap::new()),
            stop: CancellationToken::new(),
            details: Semaphore::new(4),
        })
    }

    fn prepare(&self, command: Command, metadata: Option<&acp::Meta>) -> acp::Result<Request> {
        let mut request = self.client.prepare(command);
        if let Some(id) = member(metadata, "request_id")? {
            request.id = id
                .as_str()
                .ok_or_else(|| invalid("request_id must be a string"))?
                .parse()
                .map_err(|_| invalid("request_id is invalid"))?;
        }
        Ok(request)
    }

    async fn execute(
        &self,
        command: Command,
        metadata: Option<&acp::Meta>,
    ) -> acp::Result<(Output, RequestId)> {
        let request = self.prepare(command, metadata)?;
        let id = request.id;
        self.client
            .execute(request)
            .await
            .map(|output| (output, id))
            .map_err(|error| fault(error, Some(id)))
    }

    async fn opened(&self, id: &acp::SessionId) -> acp::Result<Arc<Opened>> {
        let session = parse_session(id)?;
        self.sessions
            .lock()
            .await
            .get(&session)
            .cloned()
            .ok_or_else(|| invalid("load or create this session first"))
    }
}

pub(crate) async fn run(
    arguments: impl Iterator<Item = OsString>,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut arguments = arguments;
    let mut profile = None;
    while let Some(argument) = arguments.next() {
        if (argument == "--help" || argument == "-h") && arguments.next().is_none() {
            println!(
                "Usage: sailry acp [--data-dir <absolute profile>]\nConnects an ACP editor to the running Host over stdio. Models, workspace ownership and approvals stay in that Node."
            );
            return Ok(());
        }
        if argument != "--data-dir" || profile.is_some() {
            return Err("unknown ACP option; use --help".into());
        }
        let path = PathBuf::from(arguments.next().ok_or("--data-dir requires a path")?);
        if !path.is_absolute() {
            return Err("--data-dir must be absolute".into());
        }
        profile = Some(path);
    }
    let profile = match profile {
        Some(profile) => profile,
        None => sailry_node_runtime::default_data_dir()?,
    };
    let socket = crate::control::transport::Socket::connect(&profile).await?;
    let adapter = Adapter::new(Arc::new(Client::new(Arc::new(socket))));
    let result = connection::serve(adapter.clone(), sdk::Stdio::new()).await;
    adapter.stop.cancel();
    match result {
        Err(error) if sdk::is_incoming_transport_closed(&error) => Ok(()),
        result => result.map_err(Into::into),
    }
}

fn parse_session(id: &acp::SessionId) -> acp::Result<SessionId> {
    id.to_string()
        .parse()
        .map_err(|_| invalid("session identifier is invalid"))
}

fn member<'a>(metadata: Option<&'a acp::Meta>, name: &str) -> acp::Result<Option<&'a Value>> {
    let Some(value) = metadata.and_then(|metadata| metadata.get("sailry")) else {
        return Ok(None);
    };
    let fields = value
        .as_object()
        .ok_or_else(|| invalid("Sailry metadata must be an object"))?;
    Ok(fields.get(name))
}

fn metadata(value: Value) -> acp::Meta {
    let mut metadata = acp::Meta::new();
    metadata.insert("sailry".into(), value);
    metadata
}

fn invalid(message: &str) -> sdk::Error {
    fault(Fault::invalid(message), None)
}

fn captured(error: sdk::Error, context: Value) -> sdk::Error {
    let detail = error.data.clone();
    error.data(json!({"sailry":context,"detail":detail}))
}

fn fault(error: Fault, request: Option<RequestId>) -> sdk::Error {
    let code = if error.code == ErrorCode::InvalidRequest {
        -32602
    } else {
        -32000
    };
    sdk::Error::new(code, error.message.clone())
        .data(json!({"sailry":{"code":error.code,"request_id":request}}))
}

fn notify(
    connection: &ConnectionTo<sdk::Client>,
    session: &acp::SessionId,
    update: acp::SessionUpdate,
    context: Value,
) -> acp::Result<()> {
    connection.send_notification(
        acp::SessionNotification::new(session.clone(), update).meta(metadata(context)),
    )
}
