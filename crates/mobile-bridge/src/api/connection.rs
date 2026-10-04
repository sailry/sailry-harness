use super::*;
use sailry_client::{Client, View};
use sailry_link::Transport;
use std::sync::Arc;
use tokio::{sync::watch, task::JoinHandle};

#[frb(opaque)]
pub struct Connection {
    client: Arc<Client>,
    stop: CancellationToken,
    inbox: Arc<Mutex<sailry_client::activity::Inbox>>,
}
impl Connection {
    pub(super) fn new(
        transport: Arc<dyn Transport>,
        stop: CancellationToken,
        inbox: Arc<Mutex<sailry_client::activity::Inbox>>,
    ) -> Self {
        Self {
            client: Arc::new(Client::new(transport)),
            stop,
            inbox,
        }
    }

    /// Keep this serialized request unchanged when retrying an uncertain result.
    pub fn prepare(&self, command: String) -> Result<String, String> {
        let command = serde_json::from_str(&command).map_err(error)?;
        serde_json::to_string(&self.client.prepare(command)).map_err(error)
    }

    pub async fn execute(&self, request: String) -> Result<String, String> {
        if self.stop.is_cancelled() {
            return Err("connection is closed".into());
        }
        let request = serde_json::from_str(&request).map_err(error)?;
        // Cancellation stops observation, not the Node's admitted operation.
        let result = tokio::select! {
            result = self.client.execute(request) => result,
            _ = self.stop.cancelled() => Err(sailry_protocol::Fault::new(
                sailry_protocol::ErrorCode::OutcomeUnknown, "connection closed; retain the request identifier")),
        };
        serde_json::to_string(&result).map_err(error)
    }

    pub async fn watch(&self) -> Result<Updates, String> {
        if self.stop.is_cancelled() {
            return Err("connection is closed".into());
        }
        let stop = self.stop.child_token();
        let (sender, receiver) = watch::channel(View::default());
        let client = self.client.clone();
        let cancellation = stop.clone();
        let task = tokio::spawn(async move {
            let _ = client.watch(sender, cancellation).await;
        });
        Ok(Updates {
            receiver: Mutex::new(receiver),
            stop,
            task,
            inbox: self.inbox.clone(),
        })
    }

    pub fn close(&self) {
        self.stop.cancel();
    }

    /// Bind a controller loopback port using the shared authenticated Client.
    pub async fn forward_port(
        &self,
        remote_port: u16,
        local_port: u16,
    ) -> Result<Forwarding, String> {
        if self.stop.is_cancelled() {
            return Err("connection is closed".into());
        }
        let forwarder = tokio::select! {
            biased;
            _ = self.stop.cancelled() => return Err("connection is closed".into()),
            result = self.client.forward_port(remote_port, local_port) => result.map_err(error)?,
        };
        Ok(Forwarding::new(forwarder, self.stop.child_token()))
    }

    pub async fn watch_commands(&self, session: String) -> Result<CommandUpdates, String> {
        if self.stop.is_cancelled() {
            return Err("connection is closed".into());
        }
        let session = serde_json::from_value(serde_json::Value::String(session)).map_err(error)?;
        Ok(CommandUpdates::new(
            self.client.clone(),
            session,
            self.stop.child_token(),
        ))
    }

    pub async fn forward_service(
        &self,
        source: String,
        local_port: u16,
    ) -> Result<Forwarding, String> {
        let source = serde_json::from_str(&source).map_err(error)?;
        let forwarder = tokio::select! {
            biased;
            _ = self.stop.cancelled() => return Err("connection is closed".into()),
            result = self.client.forward_service(source, local_port) => result.map_err(error)?,
        };
        Ok(Forwarding::new(forwarder, self.stop.child_token()))
    }

    /// Adapt a Node upload descriptor to bounded byte writes; this does not publish it.
    pub async fn upload_attachment(&self, upload: String) -> Result<Upload, String> {
        if self.stop.is_cancelled() {
            return Err("connection is closed".into());
        }
        let upload = serde_json::from_str(&upload).map_err(error)?;
        Ok(Upload::new(
            self.client.clone(),
            upload,
            self.stop.child_token(),
        ))
    }

    /// Partial bytes are unverified until Download.next returns None successfully.
    pub async fn download_attachment(&self, download: String) -> Result<Download, String> {
        if self.stop.is_cancelled() {
            return Err("connection is closed".into());
        }
        let download = serde_json::from_str(&download).map_err(error)?;
        Ok(Download::new(
            self.client.clone(),
            transfers::download::Source::Attachment(download),
            self.stop.child_token(),
        ))
    }

    /// Adapt a project file descriptor to the same verified download stream.
    pub async fn download_file(&self, download: String) -> Result<Download, String> {
        if self.stop.is_cancelled() {
            return Err("connection is closed".into());
        }
        let download = serde_json::from_str(&download).map_err(error)?;
        Ok(Download::new(
            self.client.clone(),
            transfers::download::Source::File(download),
            self.stop.child_token(),
        ))
    }

    pub async fn watch_terminal(&self, terminal: String) -> Result<TerminalUpdates, String> {
        if self.stop.is_cancelled() {
            return Err("connection is closed".into());
        }
        let terminal =
            serde_json::from_value(serde_json::Value::String(terminal)).map_err(error)?;
        Ok(TerminalUpdates::new(
            self.client.clone(),
            terminal,
            self.stop.child_token(),
        ))
    }

    pub async fn watch_conversation(&self, session: String) -> Result<ConversationUpdates, String> {
        if self.stop.is_cancelled() {
            return Err("connection is closed".into());
        }
        let session = serde_json::from_value(serde_json::Value::String(session)).map_err(error)?;
        Ok(ConversationUpdates::new(
            self.client.clone(),
            session,
            self.stop.child_token(),
        ))
    }

    pub async fn watch_usage(&self, query: String) -> Result<UsageUpdates, String> {
        if self.stop.is_cancelled() {
            return Err("connection is closed".into());
        }
        let query = serde_json::from_str(&query).map_err(error)?;
        Ok(UsageUpdates::new(
            self.client.clone(),
            query,
            self.stop.child_token(),
        ))
    }

    /// Observe an existing attempt; BeginProviderLogin remains an explicit durable command.
    pub async fn watch_login(&self, attempt: String) -> Result<LoginUpdates, String> {
        if self.stop.is_cancelled() {
            return Err("connection is closed".into());
        }
        let attempt = serde_json::from_str(&attempt).map_err(error)?;
        Ok(LoginUpdates::new(
            self.client.clone(),
            attempt,
            self.stop.child_token(),
        ))
    }

    /// Observe an existing MCP OAuth attempt without initiating authorization.
    pub async fn watch_mcp_login(&self, attempt: String) -> Result<LoginUpdates, String> {
        if self.stop.is_cancelled() {
            return Err("connection is closed".into());
        }
        let attempt = serde_json::from_str(&attempt).map_err(error)?;
        Ok(LoginUpdates::mcp(
            self.client.clone(),
            attempt,
            self.stop.child_token(),
        ))
    }

    /// Search committed text through the shared Client; closing cancels this read-only query.
    pub async fn search_conversation(
        &self,
        session: String,
        query: String,
    ) -> Result<String, String> {
        if self.stop.is_cancelled() {
            return Err("connection is closed".into());
        }
        let session = serde_json::from_value(serde_json::Value::String(session)).map_err(error)?;
        let query = serde_json::from_str(&query).map_err(error)?;
        let result = tokio::select! {
            biased;
            _ = self.stop.cancelled() => return Err("connection is closed".into()),
            result = self.client.search_conversation(session, query) => result,
        };
        serde_json::to_string(&result).map_err(error)
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

#[frb(opaque)]
pub struct Updates {
    receiver: Mutex<watch::Receiver<View>>,
    stop: CancellationToken,
    task: JoinHandle<()>,
    inbox: Arc<Mutex<sailry_client::activity::Inbox>>,
}
impl Updates {
    pub async fn next(&self) -> Result<String, String> {
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => Err("subscription is closed".into()),
            result = async {
                let mut receiver = self.receiver.lock().await;
                receiver.changed().await.map_err(error)?;
                let view = receiver.borrow_and_update().clone();
                if let Some(snapshot) = &view.snapshot {
                    self.inbox.lock().await.merge(snapshot.node, &view.notifications);
                }
                serde_json::to_string(&view).map_err(error)
            } => result,
        }
    }
    pub fn close(&self) {
        self.stop.cancel();
    }
}
impl Drop for Updates {
    fn drop(&mut self) {
        self.stop.cancel();
        self.task.abort();
    }
}
