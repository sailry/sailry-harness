//! Session control uses the captured scope and the shared Client projection.
use super::*;
use sailry_protocol::{SessionId, TurnId};
use std::sync::OnceLock;
use tokio::sync::{mpsc, watch};

pub(in crate::plugins::host) struct Subscription {
    pub(in crate::plugins::host) receiver:
        OnceLock<watch::Receiver<sailry_client::conversation::View>>,
}

impl Default for Subscription {
    fn default() -> Self {
        Self {
            receiver: OnceLock::new(),
        }
    }
}

impl Host {
    pub(super) fn conversation_module(self: &Arc<Self>, module: HostModule) -> HostModule {
        let session = self.clone();
        let history = self.clone();
        let runs = self.clone();
        let run = self.clone();
        let send = self.clone();
        let stop = self.clone();
        let configure = self.clone();
        let execution = self.clone();
        let observe = self.clone();
        module
            .async_function("readSession", move |args| {
                session.read_scoped(
                    Command::ReadSession {
                        session: session.target_session(args.get(0))?,
                    },
                    true,
                )
            })
            .async_function("readConversation", move |args| {
                history.read_scoped(
                    Command::ReadConversation {
                        session: history.target_session(args.get(2))?,
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
                    },
                    true,
                )
            })
            .async_function("readConversationRuns", move |args| {
                runs.read_scoped(
                    Command::ReadConversationRuns {
                        session: runs.target_session(args.get(2))?,
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
                            .unwrap_or(100)
                            .try_into()
                            .map_err(|_| HostError::new("invalid run page limit"))?,
                    },
                    true,
                )
            })
            .async_function("readConversationRun", move |args| {
                run.read_scoped(
                    Command::ReadConversationRun {
                        session: run.target_session(args.get(2))?,
                        turn: args.string(0)?.parse().map_err(HostError::new)?,
                        expected_revision: args
                            .get(1)
                            .filter(|value| !matches!(value, HostValue::Null))
                            .map(|value| {
                                value
                                    .as_str()
                                    .ok_or_else(|| {
                                        HostError::new("revision must be a decimal string")
                                    })
                                    .and_then(revision)
                            })
                            .transpose()?,
                    },
                    true,
                )
            })
            .function("sendMessage", move |args| {
                let message = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                send.prepare_public(Command::QueueTurn {
                    session: send.target_session(args.get(2))?,
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
                    session: configure.target_session(args.get(2))?,
                    expected_revision: revision(args.string(1)?)?,
                    config,
                })
            })
            .function("setSessionExecution", move |args| {
                let mut selection = decode(args.value(0)?)?;
                let object = selection
                    .as_object_mut()
                    .ok_or_else(|| HostError::new("invalid execution selection"))?;
                if object
                    .keys()
                    .any(|key| !matches!(key.as_str(), "execution" | "team" | "models"))
                {
                    return Err(HostError::new("invalid execution selection"));
                }
                object.entry("team").or_insert(Value::Null);
                object.entry("models").or_insert(json!({}));
                object.insert(
                    "session".into(),
                    json!(execution.target_session(args.get(2))?),
                );
                object.insert("expected_revision".into(), json!(args.string(1)?));
                execution.prepare_public(values::command(
                    json!({"kind":"set_session_team","data":selection}),
                )?)
            })
            .async_function("nextConversation", move |args| {
                let projection = match args.get(1) {
                    None => Projection::Full,
                    Some(value) if value.as_str() == Some("runs") => Projection::Runs,
                    _ => return Err(HostError::new("invalid conversation projection")),
                };
                observe.next_conversation(
                    args.get(0).and_then(HostValue::as_str).unwrap_or("").into(),
                    projection,
                )
            })
    }

    fn session(&self) -> Result<SessionId, HostError> {
        self.check()?;
        self.context
            .session
            .ok_or_else(|| HostError::new("plugin has no session scope"))
    }

    fn target_session(&self, value: Option<&HostValue>) -> Result<SessionId, HostError> {
        self.check()?;
        match value {
            None | Some(HostValue::Null) => self.session(),
            Some(value) => value
                .as_str()
                .ok_or_else(|| HostError::new("invalid session ID"))?
                .parse()
                .map_err(HostError::new),
        }
    }

    fn next_conversation(
        self: &Arc<Self>,
        seen: String,
        projection: Projection,
    ) -> Result<
        impl Future<Output = Result<HostValue, HostError>> + Send + 'static + use<>,
        HostError,
    > {
        let session = self.session()?;
        let permission = self.read_public(Command::ReadSession { session })?;
        let owner = self.clone();
        Ok(async move {
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
            let mut waited = false;
            loop {
                owner.check()?;
                let value = projection.read(&updates.borrow_and_update())?;
                let bytes = serde_json::to_vec(&value)
                    .map_err(|error| HostError::new(error.to_string()))?;
                let cursor = blake3::hash(&bytes).to_hex().to_string();
                if cursor != seen {
                    if waited {
                        // A long poll may outlive package revocation. Recheck only
                        // when selected data is ready, never for filtered deltas.
                        owner.read_public(Command::ReadSession { session })?.await?;
                        owner.check()?;
                    }
                    return encode(json!({"cursor":cursor, "view":value}));
                }
                tokio::select! {
                    biased;
                    _ = owner.stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                    result = updates.changed() => {
                        result.map_err(|_| HostError::new("plugin conversation subscription ended"))?;
                        waited = true;
                    },
                }
            }
        })
    }
}

#[derive(Clone, Copy)]
enum Projection {
    Full,
    Runs,
}

impl Projection {
    fn read(self, view: &sailry_client::conversation::View) -> Result<Value, HostError> {
        match self {
            Self::Full => {
                serde_json::to_value(view).map_err(|error| HostError::new(error.to_string()))
            }
            Self::Runs => Ok(json!({
                "connected": view.connected,
                "error": view.error,
                "generation": view.generation,
                "snapshot": view.snapshot.as_ref().map(|snapshot| json!({
                    "page": {
                        "session": snapshot.page.session,
                        "runs": snapshot.page.runs.iter().map(|run| json!({
                            "turn": run.turn, "status": run.status, "error": run.error,
                        })).collect::<Vec<_>>(),
                    },
                })),
            })),
        }
    }
}
