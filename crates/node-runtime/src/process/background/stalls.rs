//! Silent background commands keep running; their owning session decides what to do.
use super::*;
use std::time::{Duration, Instant};

/// A running background command without output for this long needs a decision.
pub(crate) const STALL_WINDOW: Duration = Duration::from_secs(15 * 60);

/// One silent period of a still-running background command.
pub(crate) struct Stalled {
    pub snapshot: Snapshot,
    pub elapsed_ms: u64,
}

impl Commands {
    /// Reports each silent period of a running background command once.
    pub(crate) fn set_stall_sink(
        self: &Arc<Self>,
        sink: tokio::sync::mpsc::UnboundedSender<Stalled>,
    ) {
        assert!(
            self.stalls.set(sink).is_ok(),
            "command stall sink was already set"
        );
        tokio::spawn(monitor(
            Arc::downgrade(self),
            self.changes.subscribe(),
            self.resumed.clone(),
        ));
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_stall_window(&self, window: Duration) {
        *self.stall_window.lock().unwrap() = Some(window);
        self.changes.send_replace(());
    }

    fn stall_window(&self) -> Duration {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(window) = *self.stall_window.lock().unwrap() {
            return window;
        }
        STALL_WINDOW
    }

    /// Reports newly silent commands and returns the next deadline to inspect.
    ///
    /// Unspawned and already reported commands have no deadline; their next
    /// activity wakes the monitor once through `resumed`.
    fn inspect(&self) -> Option<Instant> {
        let sink = self.stalls.get()?;
        let window = self.stall_window();
        let now = Instant::now();
        let mut next: Option<Instant> = None;
        let mut entries = self.entries.lock().unwrap();
        for entry in entries
            .values_mut()
            .filter(|entry| entry.background && entry.info.status == Status::Running)
        {
            let updated = entry.retained.updated();
            let Some(since) = updated.filter(|since| entry.stalled != Some(*since)) else {
                entry.retained.wake(updated, &self.resumed);
                continue;
            };
            let deadline = since + window;
            if deadline > now {
                next = Some(next.map_or(deadline, |next| next.min(deadline)));
                continue;
            }
            entry.stalled = Some(since);
            entry.retained.wake(updated, &self.resumed);
            let (stdout, stderr) = entry.retained.recent();
            // Reporting under the entries lock never follows a terminal status.
            let _ = sink.send(Stalled {
                elapsed_ms: elapsed(entry.info.started_ms),
                snapshot: Snapshot {
                    info: entry.info.clone(),
                    stdout,
                    stderr,
                },
            });
        }
        next
    }
}

fn elapsed(started_ms: u64) -> u64 {
    (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64)
        .saturating_sub(started_ms)
}

async fn monitor(
    commands: std::sync::Weak<Commands>,
    mut changes: watch::Receiver<()>,
    resumed: Arc<tokio::sync::Notify>,
) {
    loop {
        let Some(next) = commands.upgrade().map(|commands| commands.inspect()) else {
            return;
        };
        let deadline = async {
            match next {
                Some(next) => tokio::time::sleep_until(next.into()).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            changed = changes.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            () = resumed.notified() => {}
            () = deadline => {}
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use sailry_protocol::process::{Access, Outcome};
    use tokio::sync::mpsc;

    struct Fixture {
        root: tempfile::TempDir,
        commands: Arc<Commands>,
        stalls: mpsc::UnboundedReceiver<Stalled>,
        finished: mpsc::UnboundedReceiver<Finished>,
        session: SessionId,
    }

    impl Fixture {
        fn new(window: Duration) -> Self {
            let commands = Arc::new(Commands::default());
            commands.set_stall_window(window);
            let (sink, stalls) = mpsc::unbounded_channel();
            commands.set_stall_sink(sink);
            let (sink, finished) = mpsc::unbounded_channel();
            commands.set_sink(sink);
            Self {
                root: tempfile::tempdir().unwrap(),
                commands,
                stalls,
                finished,
                session: SessionId::new(),
            }
        }

        fn start(&self, command: &str) -> RequestId {
            self.commands
                .start(
                    RequestId::new(),
                    self.session,
                    TurnId::new(),
                    Launch {
                        pty: false,
                        profile: None,
                        access: Access::Workspace,
                        root: self.root.path().canonicalize().unwrap(),
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
                .unwrap()
                .id
        }

        async fn stalled(&mut self) -> Stalled {
            tokio::time::timeout(Duration::from_secs(10), self.stalls.recv())
                .await
                .expect("stall was not reported")
                .unwrap()
        }
    }

    #[tokio::test]
    async fn reports_each_silence_once() {
        let mut fixture = Fixture::new(Duration::from_millis(300));
        let id = fixture.start("cat");
        let first = fixture.stalled().await;
        assert_eq!(first.snapshot.info.id, id);
        assert_eq!(first.snapshot.info.status, Status::Running);
        assert_eq!(first.snapshot.stdout.text, "");
        tokio::time::sleep(Duration::from_millis(900)).await;
        assert!(fixture.stalls.try_recv().is_err());
        // Output after a report re-arms the monitor for one more silent period.
        fixture
            .commands
            .write(fixture.session, id, "resumed\n")
            .unwrap();
        let second = fixture.stalled().await;
        assert_eq!(second.snapshot.info.id, id);
        assert_eq!(second.snapshot.info.status, Status::Running);
        assert_eq!(second.snapshot.stdout.text, "resumed\n");
        tokio::time::sleep(Duration::from_millis(900)).await;
        assert!(fixture.stalls.try_recv().is_err());
        fixture.commands.stop(fixture.session, id).unwrap();
        fixture.commands.close().await;
    }

    #[tokio::test]
    async fn completion_never_stalls() {
        let mut fixture = Fixture::new(Duration::from_secs(2));
        let id = fixture.start("printf done");
        let finished = tokio::time::timeout(Duration::from_secs(10), fixture.finished.recv())
            .await
            .expect("command did not finish")
            .unwrap();
        assert_eq!(finished.snapshot.info.id, id);
        assert_eq!(
            finished.snapshot.info.status,
            Status::Finished(Outcome::Exited(0))
        );
        tokio::time::sleep(Duration::from_millis(2500)).await;
        assert!(fixture.stalls.try_recv().is_err());
        fixture.commands.close().await;
    }
}
