//! Session-owned commands outlive Agent turns, but never the execution Node.
mod services;
mod subscription;

use super::{Environment, Launch, execute_background};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault, RequestId, SessionId, TurnId,
    process::{Capture, Info, Snapshot, Status},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{sync::watch, task::JoinHandle};

struct Entry {
    info: Info,
    output: watch::Receiver<(Capture, Capture)>,
    stop: CancellationToken,
    task: Option<JoinHandle<()>>,
    watch: Option<(sailry_protocol::WorktreeId, Arc<crate::code_index::Service>)>,
    input: Option<tokio::sync::mpsc::Sender<Vec<u8>>>,
    explicit: bool,
}

pub(crate) struct Finished {
    pub snapshot: Snapshot,
    pub elapsed_ms: Option<u64>,
    pub explicit: bool,
}

pub(crate) struct Commands {
    sink: OnceLock<tokio::sync::mpsc::UnboundedSender<Finished>>,
    entries: Mutex<BTreeMap<RequestId, Entry>>,
    changes: watch::Sender<()>,
}

impl Default for Commands {
    fn default() -> Self {
        Self {
            sink: OnceLock::new(),
            entries: Mutex::default(),
            changes: watch::channel(()).0,
        }
    }
}

impl Commands {
    pub(crate) fn set_sink(&self, sink: tokio::sync::mpsc::UnboundedSender<Finished>) {
        assert!(
            self.sink.set(sink).is_ok(),
            "command completion sink was already set"
        );
    }

    pub(crate) fn watch(
        &self,
        id: RequestId,
        worktree: sailry_protocol::WorktreeId,
        code: Arc<crate::code_index::Service>,
    ) {
        let finished = {
            let mut entries = self.entries.lock().unwrap();
            let Some(entry) = entries.get_mut(&id) else {
                return;
            };
            if matches!(entry.info.status, Status::Running | Status::Stopping) {
                entry.watch = Some((worktree, code.clone()));
                false
            } else {
                true
            }
        };
        if finished {
            code.changed(worktree, None);
        }
    }

    pub fn start(
        self: &Arc<Self>,
        id: RequestId,
        session: SessionId,
        turn: TurnId,
        launch: Launch,
        environment: Arc<Environment>,
        closed: CancellationToken,
    ) -> Result<Info, Fault> {
        let mut entries = self.entries.lock().unwrap();
        if entries
            .values()
            .filter(|entry| matches!(entry.info.status, Status::Running | Status::Stopping))
            .count()
            >= 32
        {
            return Err(Fault::new(
                ErrorCode::Busy,
                "background command capacity exhausted",
            ));
        }
        while entries.len() >= 128 {
            let oldest = entries
                .iter()
                .filter(|(_, entry)| {
                    !matches!(entry.info.status, Status::Running | Status::Stopping)
                })
                .min_by_key(|(_, entry)| entry.info.started_ms)
                .map(|(id, _)| *id);
            if let Some(id) = oldest {
                entries.remove(&id);
            } else {
                break;
            }
        }
        let info = Info {
            id,
            session,
            turn,
            command: launch.command.clone(),
            access: launch.access,
            pty: launch.pty,
            cwd: launch.cwd.clone(),
            started_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            status: Status::Running,
            services: Vec::new(),
        };
        let stop = CancellationToken::new();
        let (progress, output) = watch::channel((Capture::default(), Capture::default()));
        let (input, received) = tokio::sync::mpsc::channel(32);
        entries.insert(
            id,
            Entry {
                info: info.clone(),
                output: output.clone(),
                stop: stop.clone(),
                task: None,
                watch: None,
                input: Some(input),
                explicit: false,
            },
        );
        let owner = self.clone();
        let task = tokio::spawn(async move {
            use futures::FutureExt;
            let mut output = output;
            let execution = std::panic::AssertUnwindSafe(execute_background(
                launch,
                &environment,
                &stop,
                &closed,
                &progress,
                received,
            ))
            .catch_unwind();
            tokio::pin!(execution);
            let result = loop {
                tokio::select! {
                    result = &mut execution => break result,
                    changed = output.changed() => {
                        if changed.is_err() { continue; }
                        let (stdout, stderr) = output.borrow_and_update().clone();
                        let mut entries = owner.entries.lock().unwrap();
                        if let Some(entry) = entries.get_mut(&id) {
                            let before = entry.info.services.len();
                            services::discover(&stdout.text, &mut entry.info.services);
                            services::discover(&stderr.text, &mut entry.info.services);
                            if before != entry.info.services.len() { owner.changes.send_replace(()); }
                        }
                    }
                }
            };
            let mut elapsed_ms = None;
            let status = match result {
                Ok(Ok(result)) => {
                    elapsed_ms = Some(result.elapsed_ms);
                    progress.send_replace((result.stdout, result.stderr));
                    Status::Finished(result.outcome)
                }
                Ok(Err(error)) => Status::Failed(error.message),
                Err(_) => Status::Failed("background command execution failed".into()),
            };
            let watch = {
                let mut entries = owner.entries.lock().unwrap();
                if let Some(entry) = entries.get_mut(&id) {
                    entry.info.status = status;
                    entry.input.take();
                    entry.info.services.clear();
                    owner.changes.send_replace(());
                    if !closed.is_cancelled()
                        && let Some(sink) = owner.sink.get()
                    {
                        let (stdout, stderr) = entry.output.borrow().clone();
                        let _ = sink.send(Finished {
                            snapshot: Snapshot {
                                info: entry.info.clone(),
                                stdout,
                                stderr,
                            },
                            elapsed_ms,
                            explicit: entry.explicit,
                        });
                    }
                    entry.watch.take()
                } else {
                    None
                }
            };
            if let Some((worktree, code)) = watch {
                code.changed(worktree, None);
            }
        });
        entries.get_mut(&id).unwrap().task = Some(task);
        self.changes.send_replace(());
        Ok(info)
    }

    pub fn list(&self, session: SessionId) -> Vec<Info> {
        let mut entries: Vec<_> = self
            .entries
            .lock()
            .unwrap()
            .values()
            .filter(|entry| entry.info.session == session)
            .map(|entry| entry.info.clone())
            .collect();
        entries.sort_by_key(|entry| (entry.started_ms, entry.id));
        entries
    }

    pub fn read(&self, session: SessionId, id: RequestId) -> Result<Snapshot, Fault> {
        let entries = self.entries.lock().unwrap();
        let entry = entries
            .get(&id)
            .filter(|entry| entry.info.session == session)
            .ok_or_else(missing)?;
        let (stdout, stderr) = entry.output.borrow().clone();
        Ok(Snapshot {
            info: entry.info.clone(),
            stdout,
            stderr,
        })
    }

    pub fn stop(&self, session: SessionId, id: RequestId) -> Result<Snapshot, Fault> {
        {
            let mut entries = self.entries.lock().unwrap();
            let entry = entries
                .get_mut(&id)
                .filter(|entry| entry.info.session == session)
                .ok_or_else(missing)?;
            if entry.info.status == Status::Running {
                entry.info.status = Status::Stopping;
            }
            entry.explicit = true;
            entry.stop.cancel();
        }
        self.changes.send_replace(());
        self.read(session, id)
    }

    pub fn write(&self, session: SessionId, id: RequestId, input: &str) -> Result<usize, Fault> {
        if input.len() > 64 * 1024 {
            return Err(Fault::invalid("command input exceeds the size limit"));
        }
        let entries = self.entries.lock().unwrap();
        let entry = entries
            .get(&id)
            .filter(|entry| entry.info.session == session)
            .ok_or_else(missing)?;
        if entry.info.status != Status::Running {
            return Err(Fault::new(ErrorCode::Conflict, "command input is closed"));
        }
        if input.is_empty() {
            return Ok(0);
        }
        entry
            .input
            .as_ref()
            .ok_or_else(|| Fault::new(ErrorCode::Unavailable, "command input is closed"))?
            .try_send(input.as_bytes().to_vec())
            .map_err(|error| match error {
                tokio::sync::mpsc::error::TrySendError::Full(_) => {
                    Fault::new(ErrorCode::Busy, "command input queue is full")
                }
                tokio::sync::mpsc::error::TrySendError::Closed(_) => Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "command input closed before delivery; inspect output before retrying",
                ),
            })?;
        Ok(input.len())
    }

    pub async fn close_session(&self, session: SessionId) -> Vec<Info> {
        let closing: Vec<_> = self
            .entries
            .lock()
            .unwrap()
            .values_mut()
            .filter(|entry| entry.info.session == session)
            .map(|entry| {
                if entry.info.status == Status::Running {
                    entry.info.status = Status::Stopping;
                }
                entry.stop.cancel();
                (entry.info.id, entry.task.take())
            })
            .collect();
        let mut result = Vec::new();
        for (id, task) in closing {
            if let Some(task) = task {
                let _ = task.await;
            }
            if let Some(entry) = self.entries.lock().unwrap().remove(&id) {
                result.push(entry.info);
            }
        }
        self.changes.send_replace(());
        result
    }

    pub async fn close(&self) {
        let tasks: Vec<_> = self
            .entries
            .lock()
            .unwrap()
            .values_mut()
            .filter_map(|entry| {
                entry.stop.cancel();
                entry.task.take()
            })
            .collect();
        for task in tasks {
            let _ = task.await;
        }
    }
}

fn missing() -> Fault {
    Fault::new(
        ErrorCode::NotFound,
        "command does not belong to this session",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::process::{Access, Outcome};
    use std::time::{Duration, Instant};

    async fn finished(commands: &Commands, session: SessionId, id: RequestId) -> Snapshot {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let snapshot = commands.read(session, id).unwrap();
            if !matches!(snapshot.info.status, Status::Running | Status::Stopping) {
                return snapshot;
            }
            assert!(
                Instant::now() < deadline,
                "background command completion deadline"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn retains_scratch_until_command_exit() {
        let root = tempfile::tempdir().unwrap();
        let profile = tempfile::tempdir().unwrap();
        let controls = crate::agent::Controls::default();
        let session = SessionId::new();
        let turn = TurnId::new();
        controls.register(turn, session, CancellationToken::new());
        let scratch = controls
            .scratch(
                turn,
                profile.path(),
                sailry_protocol::WorktreeId::new(),
                session,
            )
            .unwrap();
        let path = scratch.path().to_owned();
        let commands = Arc::new(Commands::default());
        let info = commands
            .start(
                RequestId::new(),
                session,
                turn,
                Launch {
                    pty: false,
                    profile: None,
                    access: Access::Workspace,
                    root: root.path().canonicalize().unwrap(),
                    command: "test \"$TMPDIR\" = \"$TMP\" && test \"$TMPDIR\" = \"$TEMP\" || exit 1; sleep 60 & command_pid=$!; printf '%s' \"$command_pid\" > child.pid; printf started > ready.txt; wait \"$command_pid\"".into(),
                    cwd: String::new(),
                    timeout_ms: 1,
                    inputs: None,
                    scratch: Some(scratch),
                    python: None,
                },
                Arc::new(Environment::ambient()),
                CancellationToken::new(),
            )
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !root.path().join("ready.txt").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("background command did not start");
        let pid: i32 = std::fs::read_to_string(root.path().join("child.pid"))
            .unwrap()
            .parse()
            .unwrap();
        // SAFETY: signal 0 only probes this fixture child and passes no pointers.
        assert_eq!(unsafe { libc::kill(pid, 0) }, 0);
        controls.finish(turn);
        assert!(path.exists());
        commands.stop(session, info.id).unwrap();
        let output = finished(&commands, session, info.id).await;
        assert_eq!(output.info.status, Status::Finished(Outcome::Cancelled));
        assert!(!path.exists());
        let deadline = Instant::now() + Duration::from_secs(3);
        // SAFETY: signal 0 checks this fixture child without sending a signal.
        while unsafe { libc::kill(pid, 0) } == 0 {
            assert!(Instant::now() < deadline, "fixture child cleanup deadline");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        commands.close().await;
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn closes_only_owned_children() {
        let root = tempfile::tempdir().unwrap();
        let commands = Arc::new(Commands::default());
        let session = SessionId::new();
        let other = SessionId::new();
        for (owner, command) in [
            (session, "sleep 60 & printf '%s' $! > child.pid; wait"),
            (other, "sleep 60"),
        ] {
            commands
                .start(
                    RequestId::new(),
                    owner,
                    TurnId::new(),
                    Launch {
                        pty: false,
                        profile: None,
                        access: Access::Workspace,
                        root: root.path().canonicalize().unwrap(),
                        command: command.into(),
                        cwd: String::new(),
                        timeout_ms: 1,
                        inputs: None,
                        scratch: None,
                        python: None,
                    },
                    Arc::new(Environment::ambient()),
                    CancellationToken::new(),
                )
                .unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        let pid = loop {
            if let Ok(value) = std::fs::read_to_string(root.path().join("child.pid"))
                && let Ok(pid) = value.parse::<i32>()
            {
                break pid;
            }
            assert!(Instant::now() < deadline, "child startup deadline");
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        // SAFETY: signal 0 only probes this fixture PID and passes no pointers.
        assert_eq!(unsafe { libc::kill(pid, 0) }, 0);
        let stopped = commands.close_session(session).await;
        assert_eq!(stopped.len(), 1);
        assert_eq!(stopped[0].status, Status::Finished(Outcome::Cancelled));
        assert!(commands.list(session).is_empty());
        // SAFETY: signal 0 checks whether the fixture exited without sending a signal.
        while unsafe { libc::kill(pid, 0) } == 0 {
            assert!(Instant::now() < deadline, "child cleanup deadline");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(commands.list(other)[0].status, Status::Running);
        commands.close().await;
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn retains_closed_group_output() {
        let root = tempfile::tempdir().unwrap();
        let commands = Arc::new(Commands::default());
        let session = SessionId::new();
        let start = |text: &str| {
            commands
                .start(
                    RequestId::new(),
                    session,
                    TurnId::new(),
                    Launch {
                        pty: false,
                        profile: None,
                        access: Access::Workspace,
                        root: root.path().canonicalize().unwrap(),
                        command: text.into(),
                        cwd: String::new(),
                        timeout_ms: 1,
                        inputs: None,
                        scratch: None,
                        python: None,
                    },
                    Arc::new(Environment::ambient()),
                    CancellationToken::new(),
                )
                .unwrap()
        };
        let failed = start("printf 'stdout ready'; printf 'stderr ready' >&2; exit 7");
        let output = finished(&commands, session, failed.id).await;
        assert_eq!(output.info.status, Status::Finished(Outcome::Exited(7)));
        assert_eq!(output.stdout.text, "stdout ready");
        assert_eq!(output.stderr.text, "stderr ready");
        let running = start("printf 'still running'; sleep 60");
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert_eq!(
            commands.read(session, running.id).unwrap().info.status,
            Status::Running
        );
        assert_eq!(commands.list(SessionId::new()), Vec::new());
        assert_eq!(
            commands
                .stop(SessionId::new(), running.id)
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        commands.close().await;
        assert_eq!(
            commands.read(session, running.id).unwrap().info.status,
            Status::Finished(Outcome::Cancelled)
        );
    }
}
