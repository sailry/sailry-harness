mod activity;
mod process;
mod session;
pub(crate) mod shells;
pub(crate) mod tools;
mod vt;
pub(crate) use process::{Source, ssh::Connection as SshConnection};

use sailry_link::{CancellationToken, Pending, Subscription};
use sailry_protocol::{
    terminal::{Frame, Info, Launch, Status},
    *,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;

type Changed = Arc<dyn Fn(Info) + Send + Sync>;

pub(crate) struct Terminals {
    node: NodeId,
    sessions: Mutex<BTreeMap<TerminalId, Arc<session::Handle>>>,
    stopping: CancellationToken,
    changed: Changed,
}

impl Terminals {
    pub fn new(node: NodeId, changed: impl Fn(Info) + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            node,
            sessions: Mutex::new(BTreeMap::new()),
            stopping: CancellationToken::new(),
            changed: Arc::new(changed),
        })
    }

    pub fn create(&self, caller: NodeId, launch: &Launch, source: Source) -> Result<Info, Fault> {
        self.start(
            caller,
            Some(launch.worktree),
            &launch.viewport,
            &launch.appearance,
            source,
            None,
        )
    }

    pub fn create_ssh(
        &self,
        caller: NodeId,
        launch: &ssh::TerminalLaunch,
        source: Source,
    ) -> Result<Info, Fault> {
        self.start(
            caller,
            None,
            &launch.viewport,
            &launch.appearance,
            source,
            None,
        )
    }

    pub fn existing(&self, id: TerminalId) -> Result<Option<Info>, Fault> {
        if self.stopping.is_cancelled() {
            return Err(unavailable());
        }
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .get(&id)
            .map(|handle| handle.snapshot.lock().unwrap().info.clone()))
    }

    pub fn open(
        &self,
        caller: NodeId,
        previous: &Info,
        viewport: &terminal::Viewport,
        appearance: &terminal::Appearance,
        source: Source,
    ) -> Result<Info, Fault> {
        self.start(
            caller,
            previous.worktree,
            viewport,
            appearance,
            source,
            Some(previous),
        )
    }

    fn start(
        &self,
        caller: NodeId,
        worktree: Option<WorktreeId>,
        viewport: &terminal::Viewport,
        appearance: &terminal::Appearance,
        source: Source,
        previous: Option<&Info>,
    ) -> Result<Info, Fault> {
        viewport.validate()?;
        let mut sessions = self.sessions.lock().unwrap();
        if self.stopping.is_cancelled() {
            return Err(unavailable());
        }
        if let Some(previous) = previous
            && let Some(handle) = sessions.get(&previous.id)
        {
            return Ok(handle.snapshot.lock().unwrap().info.clone());
        }
        if sessions
            .values()
            .filter(|session| session.running())
            .count()
            >= 32
        {
            return Err(Fault::new(ErrorCode::Busy, "terminal capacity exhausted"));
        }
        let handle = Arc::new(session::Handle::start(
            session::Origin {
                node: self.node,
                caller,
                worktree,
            },
            viewport.clone(),
            appearance.clone(),
            source,
            self.changed.clone(),
            previous.map(|info| (info.id, info.revision + 1)),
        )?);
        let info = handle.snapshot.lock().unwrap().info.clone();
        sessions.insert(info.id, handle);
        Ok(info)
    }

    fn get(&self, id: TerminalId) -> Result<Arc<session::Handle>, Fault> {
        if self.stopping.is_cancelled() {
            return Err(unavailable());
        }
        self.sessions
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| Fault::new(ErrorCode::NotFound, "terminal is not running on this Node"))
    }

    pub fn close(&self, worktree: WorktreeId, id: TerminalId) -> Result<Info, Fault> {
        let handle = self.get(id)?;
        if handle.snapshot.lock().unwrap().info.worktree != Some(worktree) {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "terminal belongs to another worktree",
            ));
        }
        self.close_handle(id, handle)
    }

    pub fn close_ssh(&self, id: TerminalId) -> Result<Info, Fault> {
        let handle = self.get(id)?;
        if handle.snapshot.lock().unwrap().info.ssh.is_none() {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "terminal is not an SSH connection",
            ));
        }
        self.close_handle(id, handle)
    }

    fn close_handle(&self, id: TerminalId, handle: Arc<session::Handle>) -> Result<Info, Fault> {
        handle.close();
        let mut snapshot = handle.snapshot.lock().unwrap();
        snapshot.info.status = Status::Closed;
        snapshot.info.owner = None;
        snapshot.info.revision += 1;
        snapshot.sequence += 1;
        let info = snapshot.info.clone();
        let _ = handle.events.send(Frame {
            node: self.node,
            info: info.clone(),
            sequence: snapshot.sequence,
            screen: terminal::ScreenUpdate::Replace {
                screen: snapshot.screen.clone(),
            },
        });
        (self.changed)(info.clone());
        drop(snapshot);
        self.sessions.lock().unwrap().remove(&id);
        Ok(info)
    }

    pub fn active(&self, worktree: WorktreeId) -> bool {
        self.sessions.lock().unwrap().values().any(|handle| {
            let snapshot = handle.snapshot.lock().unwrap();
            snapshot.info.worktree == Some(worktree) && snapshot.info.status == Status::Running
        })
    }

    pub async fn command(&self, caller: NodeId, command: Command) -> Result<Output, Fault> {
        let id = match &command {
            Command::InspectTerminal { terminal }
            | Command::ClaimTerminal { terminal, .. }
            | Command::InputTerminal { terminal, .. }
            | Command::ResizeTerminal { terminal, .. }
            | Command::SetTerminalAppearance { terminal, .. } => *terminal,
            _ => {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "terminal command expected",
                ));
            }
        };
        let handle = self.get(id)?;
        if matches!(command, Command::InspectTerminal { .. }) {
            return Ok(Output::TerminalSnapshot(
                handle.snapshot.lock().unwrap().clone(),
            ));
        }
        handle.command(caller, command).await
    }

    pub fn subscribe(&self, id: TerminalId) -> Result<Box<dyn Subscription>, Fault> {
        let handle = self.get(id)?;
        // Queue before snapshot so the sequence can discard overlapping frames.
        let events = handle.events.subscribe();
        let initial = Some(Update::TerminalSnapshot(
            handle.snapshot.lock().unwrap().clone(),
        ));
        Ok(Box::new(Frames {
            handle,
            initial,
            events,
            stopped: self.stopping.clone(),
        }))
    }

    pub fn stop(&self) {
        self.stopping.cancel();
        for handle in self.sessions.lock().unwrap().values() {
            handle.stop();
        }
    }

    pub fn join(&self) {
        self.stop();
        let sessions = std::mem::take(&mut *self.sessions.lock().unwrap());
        for handle in sessions.values() {
            handle.close();
        }
    }
}

struct Frames {
    stopped: CancellationToken,
    handle: Arc<session::Handle>,
    initial: Option<Update>,
    events: broadcast::Receiver<Frame>,
}

impl Subscription for Frames {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            if self.stopped.is_cancelled() {
                return Err(unavailable());
            }
            if let Some(initial) = self.initial.take() {
                return Ok(initial);
            }
            let frame = tokio::select! {
                biased;
                _ = self.stopped.cancelled() => return Err(unavailable()),
                frame = self.events.recv() => frame,
            };
            match frame {
                Ok(frame) => Ok(Update::TerminalFrame(frame)),
                Err(broadcast::error::RecvError::Lagged(_)) => Ok(Update::TerminalSnapshot(
                    self.handle.snapshot.lock().unwrap().clone(),
                )),
                Err(broadcast::error::RecvError::Closed) => Err(unavailable()),
            }
        })
    }
}

fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "terminal service is unavailable")
}

fn failure(error: impl std::fmt::Display) -> Fault {
    Fault::new(
        ErrorCode::Internal,
        format!("terminal operation failed: {error}"),
    )
}
