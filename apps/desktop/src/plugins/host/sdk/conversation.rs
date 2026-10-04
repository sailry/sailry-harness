//! Session control uses the captured scope and the shared Client projection.
use super::*;
use sailry_protocol::{SessionId, TurnId};
use std::sync::OnceLock;
use tokio::sync::{Semaphore, mpsc, watch};

pub(in crate::plugins::host) struct Subscription {
    receiver: OnceLock<watch::Receiver<sailry_client::conversation::View>>,
    pending: Arc<Semaphore>,
}

impl Default for Subscription {
    fn default() -> Self {
        Self {
            receiver: OnceLock::new(),
            pending: Arc::new(Semaphore::new(1)),
        }
    }
}

impl Host {
    pub(super) fn conversation_module(self: &Arc<Self>, module: HostModule) -> HostModule {
        let session = self.clone();
        let history = self.clone();
        let send = self.clone();
        let stop = self.clone();
        let configure = self.clone();
        let observe = self.clone();
        module
            .async_function("readSession", move |_| {
                session.read_public(Command::ReadSession {
                    session: session.session()?,
                })
            })
            .async_function("readConversation", move |args| {
                history.read_public(Command::ReadConversation {
                    session: history.session()?,
                    before: args
                        .get(0)
                        .and_then(HostValue::as_str)
                        .map(str::parse::<TurnId>)
                        .transpose()
                        .map_err(HostError::new)?,
                    limit: args
                        .get(1)
                        .map(|_| args.integer(1))
                        .transpose()?
                        .unwrap_or(20)
                        .try_into()
                        .map_err(|_| HostError::new("invalid history page limit"))?,
                })
            })
            .function("sendMessage", move |args| {
                let message = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                send.prepare_public(Command::QueueTurn {
                    session: send.session()?,
                    expected_revision: revision(args.string(1)?)?,
                    message,
                })
            })
            .function("stopTurn", move |args| {
                stop.session()?;
                stop.prepare_public(Command::StopTurn {
                    turn: args.string(0)?.parse().map_err(HostError::new)?,
                })
            })
            .function("setSessionConfig", move |args| {
                let config = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                configure.prepare_public(Command::SetSessionConfig {
                    session: configure.session()?,
                    expected_revision: revision(args.string(1)?)?,
                    config,
                })
            })
            .async_function("nextConversation", move |args| {
                observe
                    .next_conversation(args.get(0).and_then(HostValue::as_str).unwrap_or("").into())
            })
    }

    fn session(&self) -> Result<SessionId, HostError> {
        self.check()?;
        self.context
            .session
            .ok_or_else(|| HostError::new("plugin has no session scope"))
    }

    fn next_conversation(
        self: &Arc<Self>,
        seen: String,
    ) -> Result<
        impl Future<Output = Result<HostValue, HostError>> + Send + 'static + use<>,
        HostError,
    > {
        let session = self.session()?;
        let permission = self.read_public(Command::ReadSession { session })?;
        let permit = self
            .conversation
            .pending
            .clone()
            .try_acquire_owned()
            .map_err(|_| HostError::new("a conversation update request is already pending"))?;
        let owner = self.clone();
        Ok(async move {
            let _permit = permit;
            // The Node checks the live package and its declared read action before
            // exposing any data from the shared subscription, including cached data.
            permission.await?;
            let mut updates = owner
                .conversation
                .receiver
                .get_or_init(|| {
                    let (sender, receiver) = watch::channel(Default::default());
                    let (_, requests) = mpsc::channel(1);
                    let client = owner.client.clone();
                    let stop = owner.stop.clone();
                    owner.runtime.spawn(async move {
                        let _ = client
                            .watch_conversation(session, sender, stop, requests)
                            .await;
                    });
                    receiver
                })
                .clone();
            loop {
                owner.check()?;
                let value = serde_json::to_value(updates.borrow_and_update().clone())
                    .map_err(|error| HostError::new(error.to_string()))?;
                let bytes = serde_json::to_vec(&value)
                    .map_err(|error| HostError::new(error.to_string()))?;
                let cursor = blake3::hash(&bytes).to_hex().to_string();
                if cursor != seen {
                    return encode(json!({"cursor":cursor, "view":value}));
                }
                tokio::select! {
                    biased;
                    _ = owner.stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                    result = updates.changed() => if result.is_err() {
                        return Err(HostError::new("plugin conversation subscription ended"));
                    },
                }
            }
        })
    }
}
