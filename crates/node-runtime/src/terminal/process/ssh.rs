//! SSH PTY I/O for the existing Ghostty terminal owner.
use super::super::session::Message;
use crate::ssh::Verifier;
use russh::{Channel, ChannelMsg, Disconnect, client};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, terminal::Viewport};
use std::{
    sync::{Arc, Mutex, mpsc::SyncSender},
    time::Duration,
};
use tokio::{runtime::Handle, sync::mpsc, task::JoinHandle};

pub(crate) struct Connection {
    session: Option<client::Handle<Verifier>>,
    channel: Option<Channel<client::Msg>>,
    initial: Vec<u8>,
    runtime: Handle,
}

impl Connection {
    pub async fn open(
        session: client::Handle<Verifier>,
        viewport: &Viewport,
    ) -> Result<Self, Fault> {
        let mut connection = Self {
            session: Some(session),
            channel: None,
            initial: Vec::new(),
            runtime: Handle::current(),
        };
        let channel = connection
            .session
            .as_ref()
            .unwrap()
            .channel_open_session()
            .await
            .map_err(|_| fault("SSH terminal channel could not be opened"))?;
        connection.channel = Some(channel);
        let channel = connection.channel.as_mut().unwrap();
        channel
            .request_pty(
                true,
                "xterm-256color",
                viewport.columns.into(),
                viewport.rows.into(),
                viewport.pixel_width,
                viewport.pixel_height,
                &[],
            )
            .await
            .map_err(|_| fault("SSH terminal request was interrupted"))?;
        accepted(channel, &mut connection.initial).await?;
        channel
            .request_shell(true)
            .await
            .map_err(|_| fault("SSH shell request was interrupted"))?;
        accepted(channel, &mut connection.initial).await?;
        Ok(connection)
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        if let Some(session) = self.session.take() {
            let channel = self.channel.take();
            self.runtime.spawn(async move {
                let _ = tokio::time::timeout(Duration::from_secs(1), async {
                    if let Some(channel) = channel {
                        let _ = channel.close().await;
                    }
                    let _ = session
                        .disconnect(Disconnect::ByApplication, "", "en")
                        .await;
                })
                .await;
            });
        }
    }
}

async fn accepted(channel: &mut Channel<client::Msg>, initial: &mut Vec<u8>) -> Result<(), Fault> {
    while let Some(message) = channel.wait().await {
        match message {
            ChannelMsg::Success => return Ok(()),
            ChannelMsg::Failure => {
                return Err(Fault::new(
                    ErrorCode::Unavailable,
                    "SSH server rejected the terminal request",
                ));
            }
            ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } => {
                if initial.len() + data.len() > 128 * 1024 {
                    return Err(fault("SSH terminal startup output exceeds the limit"));
                }
                initial.extend_from_slice(&data);
            }
            ChannelMsg::Eof | ChannelMsg::Close => break,
            _ => {}
        }
    }
    Err(fault("SSH terminal closed before confirming the request"))
}

enum Action {
    Write(Vec<u8>),
    Resize(Viewport),
}

pub(in crate::terminal) struct Process {
    commands: mpsc::Sender<Action>,
    status: Arc<Mutex<Option<Result<u32, String>>>>,
    stop: CancellationToken,
    task: Option<JoinHandle<()>>,
    runtime: Handle,
}

impl Process {
    pub fn start(mut connection: Connection, messages: SyncSender<Message>) -> Self {
        let (commands, mut receiver) = mpsc::channel(64);
        let status = Arc::new(Mutex::new(None));
        let finished = status.clone();
        let stop = CancellationToken::new();
        let stopping = stop.clone();
        let runtime = connection.runtime.clone();
        let task = runtime.spawn(async move {
            let (mut reader, writer) = connection.channel.take().unwrap().split();
            let output = async {
                if !connection.initial.is_empty() {
                    forward(
                        &messages,
                        Message::Bytes(std::mem::take(&mut connection.initial)),
                    )
                    .await?;
                }
                let mut code = None;
                while let Some(message) = reader.wait().await {
                    match message {
                        ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } => {
                            forward(&messages, Message::Bytes(data.to_vec())).await?
                        }
                        ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
                        ChannelMsg::ExitSignal { .. } => {
                            return Err("SSH shell exited after a signal".into());
                        }
                        ChannelMsg::Close => break,
                        _ => {}
                    }
                }
                code.ok_or_else(|| "SSH terminal ended without an exit status".into())
            };
            let input = async {
                while let Some(action) = receiver.recv().await {
                    let result = match action {
                        Action::Write(bytes) => writer.data_bytes(bytes).await,
                        Action::Resize(viewport) => {
                            writer
                                .window_change(
                                    viewport.columns.into(),
                                    viewport.rows.into(),
                                    viewport.pixel_width,
                                    viewport.pixel_height,
                                )
                                .await
                        }
                    };
                    result.map_err(|_| {
                        "SSH terminal input was interrupted; input was not replayed".to_owned()
                    })?;
                }
                Err("SSH terminal input closed".to_owned())
            };
            let result = tokio::select! {
                _ = stopping.cancelled() => None,
                result = output => Some(result),
                result = input => Some(result),
            };
            if let Some(result) = result {
                *finished.lock().unwrap() = Some(result.clone());
                let _ = forward(&messages, Message::Eof(result.map(|_| ()))).await;
            }
            let session = connection.session.take().unwrap();
            let _ = tokio::time::timeout(Duration::from_secs(1), async {
                let _ = writer.close().await;
                let _ = session
                    .disconnect(Disconnect::ByApplication, "", "en")
                    .await;
            })
            .await;
        });
        Self {
            commands,
            status,
            stop,
            task: Some(task),
            runtime,
        }
    }

    fn send(&self, action: Action) -> Result<(), Fault> {
        self.commands.try_send(action).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => {
                Fault::new(ErrorCode::Busy, "SSH terminal input queue is full")
            }
            mpsc::error::TrySendError::Closed(_) => fault("SSH terminal connection is closed"),
        })
    }

    pub fn write(&self, bytes: Vec<u8>) -> Result<(), Fault> {
        self.send(Action::Write(bytes))
    }
    pub fn resize(&self, viewport: &Viewport) -> Result<(), Fault> {
        self.send(Action::Resize(viewport.clone()))
    }
    pub fn try_wait(&self) -> Result<Option<u32>, Fault> {
        self.status
            .lock()
            .unwrap()
            .clone()
            .transpose()
            .map_err(|message| fault(&message))
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            // The VT owner drops its receiver before joining I/O, releasing a
            // backpressured output sender. This runs on its dedicated thread.
            let _ = self.runtime.block_on(task);
        }
    }
}

async fn forward(messages: &SyncSender<Message>, message: Message) -> Result<(), String> {
    let messages = messages.clone();
    tokio::task::spawn_blocking(move || messages.send(message))
        .await
        .map_err(|_| "SSH terminal output worker failed".to_owned())?
        .map_err(|_| "SSH terminal output closed".to_owned())
}

fn fault(message: &str) -> Fault {
    Fault::new(ErrorCode::OutcomeUnknown, message)
}
