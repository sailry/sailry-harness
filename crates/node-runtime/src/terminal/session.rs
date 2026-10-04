use super::{Changed, Source, failure, process::Process, unavailable, vt::Vt};
use sailry_protocol::{terminal::*, *};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use tokio::sync::{broadcast, oneshot};

pub(super) enum Message {
    Bytes(Vec<u8>),
    Eof(Result<(), String>),
    Command {
        caller: NodeId,
        command: Box<Command>,
        reply: oneshot::Sender<Result<Output, Fault>>,
    },
}

pub(super) struct Handle {
    pub snapshot: Arc<Mutex<terminal::Snapshot>>,
    pub events: broadcast::Sender<Frame>,
    messages: SyncSender<Message>,
    stopped: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

pub(super) struct Origin {
    pub node: NodeId,
    pub caller: NodeId,
    pub worktree: Option<WorktreeId>,
}

impl Handle {
    pub fn start(
        origin: Origin,
        viewport: Viewport,
        appearance: Appearance,
        source: Source,
        changed: Changed,
        identity: Option<(TerminalId, u64)>,
    ) -> Result<Self, Fault> {
        let Origin {
            node,
            caller,
            worktree,
        } = origin;
        let (messages, receiver) = mpsc::sync_channel(64);
        let (ready, initialized) = mpsc::sync_channel(1);
        let (events, _) = broadcast::channel(64);
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let updates = events.clone();
        let output = messages.clone();
        let thread = std::thread::Builder::new()
            .name("sailry-terminal".into())
            .spawn(move || {
                // Ghostty's !Send state is constructed, used, and destroyed on this thread.
                let initialize = || -> Result<_, Fault> {
                    let vt = Vt::new(&viewport, &appearance, MAX_SCROLLBACK_ROWS)?;
                    let ssh = source.profile();
                    let tool = source.tool();
                    let process = Process::start(source, &viewport, output)?;
                    let info = Info {
                        id: identity.map_or_else(TerminalId::new, |(id, _)| id),
                        worktree,
                        ssh,
                        tool,
                        status: Status::Running,
                        activity: None,
                        title: None,
                        directory: None,
                        owner: Some(caller),
                        revision: identity.map_or(1, |(_, revision)| revision),
                    };
                    let snapshot = Arc::new(Mutex::new(terminal::Snapshot {
                        node,
                        info: info.clone(),
                        sequence: 0,
                        screen: vt.checkpoint(),
                    }));
                    changed(info);
                    Ok((vt, process, snapshot))
                };
                let (vt, process, snapshot) = match initialize() {
                    Ok(initialized) => initialized,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                if ready.send(Ok(snapshot.clone())).is_err() {
                    drop(receiver);
                    drop(process);
                    return;
                }
                let mut owner = Owner {
                    vt,
                    activity: super::activity::Tracker::default(),
                    process,
                    snapshot,
                    events: updates,
                    viewport,
                    changed,
                };
                let status = owner.run(&receiver, &stop);
                // Release blocked output producers before canceling and joining PTY I/O.
                drop(receiver);
                {
                    let mut snapshot = owner.snapshot.lock().unwrap();
                    snapshot.info.status = status;
                    snapshot.info.owner = None;
                    snapshot.info.revision += 1;
                    (owner.changed)(snapshot.info.clone());
                }
                owner.publish(ScreenUpdate::Replace {
                    screen: owner.vt.checkpoint(),
                });
            })
            .map_err(failure)?;
        match initialized.recv().map_err(failure)? {
            Ok(snapshot) => Ok(Self {
                snapshot,
                events,
                messages,
                stopped,
                thread: Mutex::new(Some(thread)),
            }),
            Err(error) => {
                let _ = thread.join();
                Err(error)
            }
        }
    }

    pub fn running(&self) -> bool {
        self.snapshot.lock().unwrap().info.status == Status::Running
    }

    pub async fn command(&self, caller: NodeId, command: Command) -> Result<Output, Fault> {
        if self.stopped.load(Ordering::Acquire) {
            return Err(unavailable());
        }
        let (reply, response) = oneshot::channel();
        self.messages
            .try_send(Message::Command {
                caller,
                command: Box::new(command),
                reply,
            })
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => {
                    Fault::new(ErrorCode::Busy, "terminal command queue is full")
                }
                mpsc::TrySendError::Disconnected(_) => unavailable(),
            })?;
        response.await.map_err(|_| {
            Fault::new(
                ErrorCode::OutcomeUnknown,
                "terminal input outcome is unknown; input was not replayed",
            )
        })?
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
    }

    pub fn close(&self) {
        self.stop();
        if let Some(thread) = self.thread.lock().unwrap().take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.close();
    }
}

struct Owner {
    vt: Vt,
    activity: super::activity::Tracker,
    process: Process,
    snapshot: Arc<Mutex<terminal::Snapshot>>,
    events: broadcast::Sender<Frame>,
    viewport: Viewport,
    changed: Changed,
}

impl Owner {
    fn run(&mut self, messages: &Receiver<Message>, stop: &AtomicBool) -> Status {
        let mut exited = None;
        let mut ended = None;
        loop {
            if stop.load(Ordering::Acquire) {
                // Node shutdown stops a process; only CloseTerminal removes its record.
                return Status::Stopped;
            }
            match messages.recv_timeout(Duration::from_millis(100)) {
                Ok(Message::Bytes(bytes)) => match self.vt.write(&bytes) {
                    Ok(write) => {
                        if let Some(activity) = self.activity.update(&bytes) {
                            let mut snapshot = self.snapshot.lock().unwrap();
                            snapshot.info.activity = Some(ActivityReport {
                                state: activity,
                                sequence: snapshot
                                    .info
                                    .activity
                                    .map_or(1, |report| report.sequence + 1),
                            });
                            (self.changed)(snapshot.info.clone());
                        }
                        for response in write.responses {
                            if let Err(error) = self.process.write(response) {
                                return Status::Failed {
                                    message: error.message,
                                };
                            }
                        }
                        if let Some(screen) = write.screen {
                            self.publish(screen);
                        }
                    }
                    Err(error) => {
                        return Status::Failed {
                            message: error.message,
                        };
                    }
                },
                Ok(Message::Command {
                    caller,
                    command,
                    reply,
                }) => {
                    let _ = reply.send(self.command(caller, *command));
                }
                Ok(Message::Eof(result)) => {
                    if let Err(message) = result {
                        return Status::Failed { message };
                    }
                    ended = Some(Instant::now());
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return Status::Closed,
            }
            match self.vt.flush(Instant::now(), ended.is_some()) {
                Ok(Some(screen)) => self.publish(screen),
                Ok(None) => {}
                Err(error) => {
                    return Status::Failed {
                        message: error.message,
                    };
                }
            }
            if exited.is_none() {
                match self.process.try_wait() {
                    Ok(Some(code)) => exited = Some((code, Instant::now())),
                    Ok(None) => {}
                    Err(error) => {
                        return Status::Failed {
                            message: error.message,
                        };
                    }
                }
            }
            if let Some((code, since)) = exited {
                if ended.is_some() || since.elapsed() >= Duration::from_millis(250) {
                    return Status::Exited { code };
                }
            } else if ended.is_some_and(|since| since.elapsed() >= Duration::from_secs(1)) {
                return Status::Failed {
                    message: "terminal output closed without an exit status".into(),
                };
            }
        }
    }

    fn command(&mut self, caller: NodeId, command: Command) -> Result<Output, Fault> {
        match command {
            Command::ClaimTerminal {
                expected_revision, ..
            } => {
                let info = {
                    let mut snapshot = self.snapshot.lock().unwrap();
                    if snapshot.info.revision != expected_revision {
                        return Err(conflict());
                    }
                    if snapshot.info.owner != Some(caller) {
                        snapshot.info.owner = Some(caller);
                        snapshot.info.revision += 1;
                    }
                    snapshot.info.clone()
                };
                (self.changed)(info.clone());
                self.publish(ScreenUpdate::Replace {
                    screen: self.vt.checkpoint(),
                });
                Ok(Output::Terminal(info))
            }
            Command::InputTerminal {
                terminal,
                revision,
                input,
            } => {
                self.check_owner(caller, revision)?;
                input
                    .validate()
                    .map_err(|error| Fault::new(ErrorCode::InvalidRequest, error.to_string()))?;
                let bytes = self.vt.encode_input(&input)?;
                self.process.write(bytes)?;
                Ok(Output::TerminalInput { terminal })
            }
            Command::ResizeTerminal {
                revision, viewport, ..
            } => {
                self.check_owner(caller, revision)?;
                viewport.validate()?;
                if viewport != self.viewport {
                    self.process.resize(&viewport)?;
                    let screen = match self.vt.resize(&viewport) {
                        Ok(screen) => screen,
                        Err(error) => {
                            self.process.resize(&self.viewport).map_err(|_| {
                                Fault::new(
                                    ErrorCode::OutcomeUnknown,
                                    "terminal resize outcome is unknown",
                                )
                            })?;
                            return Err(error);
                        }
                    };
                    self.viewport = viewport;
                    self.publish(ScreenUpdate::Replace { screen });
                }
                Ok(Output::TerminalSnapshot(
                    self.snapshot.lock().unwrap().clone(),
                ))
            }
            Command::SetTerminalAppearance {
                revision,
                appearance,
                ..
            } => {
                self.check_owner(caller, revision)?;
                let update = self.vt.appearance(&appearance)?;
                for response in update.responses {
                    self.process.write(response)?;
                }
                if let Some(screen) = update.screen {
                    self.publish(screen);
                }
                Ok(Output::TerminalSnapshot(
                    self.snapshot.lock().unwrap().clone(),
                ))
            }
            _ => Err(Fault::new(
                ErrorCode::InvalidRequest,
                "terminal control command expected",
            )),
        }
    }

    fn check_owner(&self, caller: NodeId, revision: u64) -> Result<(), Fault> {
        let snapshot = self.snapshot.lock().unwrap();
        if snapshot.info.revision != revision {
            return Err(conflict());
        }
        if snapshot.info.owner != Some(caller) {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "terminal input belongs to another controller",
            ));
        }
        Ok(())
    }

    fn publish(&self, screen: ScreenUpdate) {
        let mut snapshot = self.snapshot.lock().unwrap();
        snapshot.sequence += 1;
        snapshot.screen = self.vt.checkpoint();
        let features = &snapshot.screen.features;
        let title = (!features.title.is_empty()).then(|| features.title.clone());
        let directory = (!features.directory.is_empty()).then(|| features.directory.clone());
        if snapshot.info.title != title || snapshot.info.directory != directory {
            snapshot.info.title = title;
            snapshot.info.directory = directory;
            (self.changed)(snapshot.info.clone());
        }
        let _ = self.events.send(Frame {
            node: snapshot.node,
            info: snapshot.info.clone(),
            sequence: snapshot.sequence,
            screen,
        });
    }
}

fn conflict() -> Fault {
    Fault::new(ErrorCode::RevisionConflict, "terminal controller changed")
}
