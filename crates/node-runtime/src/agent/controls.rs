use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, SessionId, TurnId, WorktreeId};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

struct Active {
    session: SessionId,
    stop: CancellationToken,
    operations: usize,
    media: Option<super::media::Binding>,
    scratch: Option<Arc<tempfile::TempDir>>,
    python: Vec<crate::python::Access>,
    finishing: bool,
    yielded: bool,
    /// Awaiting or streaming a model response; no tool from this response has started.
    modeling: bool,
}

struct Child {
    turn: TurnId,
    stop: CancellationToken,
}

#[derive(Default)]
pub(crate) struct Controls {
    pub(super) mcp: super::mcp::Pool,
    pub(crate) wake: Notify,
    pub(crate) stopped: CancellationToken,
    active: Mutex<BTreeMap<TurnId, Active>>,
    children: Mutex<BTreeMap<(SessionId, SessionId), Child>>,
    changed: Notify,
}

impl Controls {
    pub(crate) async fn close(&self) -> Result<(), sailry_protocol::Fault> {
        self.mcp.close().await
    }
    pub fn register(&self, turn: TurnId, session: SessionId, stop: CancellationToken) {
        self.active.lock().unwrap().insert(
            turn,
            Active {
                session,
                stop,
                operations: 0,
                media: None,
                scratch: None,
                python: Vec::new(),
                finishing: false,
                yielded: false,
                modeling: false,
            },
        );
    }

    pub fn bind_media(&self, turn: TurnId, binding: super::media::Binding) {
        if let Some(active) = self.active.lock().unwrap().get_mut(&turn) {
            active.media.get_or_insert(binding);
        }
    }

    pub fn media(&self, turn: TurnId) -> Option<super::media::Binding> {
        self.active.lock().unwrap().get(&turn)?.media.clone()
    }

    pub(crate) fn stream(
        &self,
        turn: TurnId,
        session: SessionId,
        publish: impl FnOnce() -> Result<(), Fault>,
    ) -> Result<(), Fault> {
        let runs = self.active.lock().unwrap();
        let Some(active) = runs.get(&turn) else {
            return Ok(());
        };
        if active.session != session {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "stream belongs to another session",
            ));
        }
        if active.finishing || active.stop.is_cancelled() || self.stopped.is_cancelled() {
            return Ok(());
        }
        // Serialize cancellation with publication; the feed checks its active turn again.
        publish()
    }

    pub(crate) fn bind_python(
        &self,
        turn: TurnId,
        ready: crate::python::Ready,
        directory: std::path::PathBuf,
    ) -> Result<(), Fault> {
        let mut runs = self.active.lock().unwrap();
        let active = runs
            .get_mut(&turn)
            .filter(|active| !active.finishing && !active.stop.is_cancelled())
            .ok_or_else(|| Fault::new(ErrorCode::Conflict, "turn is not running"))?;
        if !active.python.iter().any(|existing| {
            existing.ready.environment == ready.environment && existing.directory == directory
        }) {
            active
                .python
                .push(crate::python::Access { ready, directory });
        }
        Ok(())
    }

    pub(crate) fn python(&self, turn: TurnId) -> Vec<crate::python::Access> {
        self.active
            .lock()
            .unwrap()
            .get(&turn)
            .map(|active| active.python.clone())
            .unwrap_or_default()
    }

    pub(crate) fn python_stop(&self, turn: TurnId) -> Option<CancellationToken> {
        self.active
            .lock()
            .unwrap()
            .get(&turn)
            .filter(|active| !active.finishing)
            .map(|active| active.stop.clone())
    }

    pub(crate) fn scratch(
        &self,
        turn: TurnId,
        profile: &Path,
        worktree: WorktreeId,
        session: SessionId,
    ) -> Result<Arc<tempfile::TempDir>, Fault> {
        let mut runs = self.active.lock().unwrap();
        let active = runs
            .get_mut(&turn)
            .filter(|active| active.session == session)
            .ok_or_else(|| Fault::new(ErrorCode::Conflict, "turn is not running"))?;
        if let Some(scratch) = &active.scratch {
            return Ok(scratch.clone());
        }
        let directory = tempfile::Builder::new()
            .prefix(&format!("command-{worktree}-{session}-{turn}-"))
            .tempdir_in(crate::profile::temporary(profile)?)
            .map_err(|error| {
                Fault::new(
                    ErrorCode::Unavailable,
                    format!("command temporary storage is unavailable: {error}"),
                )
            })?;
        let scratch = Arc::new(directory);
        active.scratch = Some(scratch.clone());
        Ok(scratch)
    }

    pub fn cancel(&self, turn: TurnId) {
        if let Some(active) = self.active.lock().unwrap().get_mut(&turn) {
            active.yielded = false;
            active.stop.cancel();
            for ((parent, _), child) in self.children.lock().unwrap().iter() {
                if *parent == active.session {
                    child.stop.cancel();
                }
            }
        }
    }

    pub fn yield_run(&self, turn: TurnId) {
        if let Some(active) = self.active.lock().unwrap().get_mut(&turn) {
            // Guidance replaces the Runner, not its session-owned delegated work.
            active.yielded |= !active.stop.is_cancelled();
            active.stop.cancel();
        }
    }

    pub(crate) fn model_started(&self, turn: TurnId) {
        if let Some(active) = self.active.lock().unwrap().get_mut(&turn) {
            active.modeling = true;
        }
    }

    /// Ends the model phase before tools run. Returns false once guidance has taken over;
    /// an explicit Stop keeps its ordinary tool-phase settlement.
    pub(crate) fn model_settled(&self, turn: TurnId) -> bool {
        let mut runs = self.active.lock().unwrap();
        let Some(active) = runs.get_mut(&turn) else {
            return true;
        };
        active.modeling = false;
        !active.yielded
    }

    /// Yields a run still in its model phase; tool batches always settle first.
    pub(crate) fn interrupt_model(
        &self,
        turn: TurnId,
        handoff: impl FnOnce() -> Result<bool, Fault>,
    ) -> Result<bool, Fault> {
        let mut runs = self.active.lock().unwrap();
        let Some(active) = runs.get_mut(&turn) else {
            return Ok(false);
        };
        if !active.modeling
            || active.finishing
            || active.stop.is_cancelled()
            || self.stopped.is_cancelled()
            || !handoff()?
        {
            return Ok(false);
        }
        active.modeling = false;
        active.yielded = true;
        active.stop.cancel();
        Ok(true)
    }

    pub fn yielded(&self, turn: TurnId) -> bool {
        self.active
            .lock()
            .unwrap()
            .get(&turn)
            .is_some_and(|active| active.yielded)
    }

    pub fn register_child(
        &self,
        parent: SessionId,
        child: SessionId,
        turn: TurnId,
        stop: CancellationToken,
    ) {
        self.children
            .lock()
            .unwrap()
            .insert((parent, child), Child { turn, stop });
        self.changed.notify_waiters();
    }

    pub fn finish_child(&self, parent: SessionId, child: SessionId, turn: TurnId) {
        let mut children = self.children.lock().unwrap();
        // An idle session can be reused after its durable completion, before
        // the previous task's cleanup guard has been released.
        if children
            .get(&(parent, child))
            .is_some_and(|active| active.turn == turn)
        {
            children.remove(&(parent, child));
        }
        self.changed.notify_waiters();
    }

    pub fn has_children(&self, parent: SessionId) -> bool {
        self.children
            .lock()
            .unwrap()
            .keys()
            .any(|(owner, _)| *owner == parent)
    }

    pub fn cancel_children(&self, parent: SessionId) {
        for ((owner, _), child) in self.children.lock().unwrap().iter() {
            if *owner == parent {
                child.stop.cancel();
            }
        }
    }

    pub fn stop(&self) {
        self.stopped.cancel();
        for active in self.active.lock().unwrap().values_mut() {
            active.yielded = false;
            active.stop.cancel();
        }
        for child in self.children.lock().unwrap().values() {
            child.stop.cancel();
        }
    }

    pub fn begin_operation(&self, turn: TurnId) -> Option<CancellationToken> {
        let mut runs = self.active.lock().unwrap();
        let active = runs.get_mut(&turn)?;
        if active.finishing {
            return None;
        }
        active.operations += 1;
        Some(active.stop.clone())
    }

    pub fn finish_operation(&self, turn: TurnId) {
        if let Some(active) = self.active.lock().unwrap().get_mut(&turn) {
            active.operations -= 1;
        }
        self.changed.notify_waiters();
    }

    pub async fn drain(&self, turn: TurnId) {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut runs = self.active.lock().unwrap();
                let Some(active) = runs.get_mut(&turn) else {
                    return;
                };
                // Sealing and registration use one lock, so no operation can enter
                // between the empty check and the durable turn completion.
                active.finishing = true;
                if active.operations == 0 {
                    return;
                }
            }
            notified.await;
        }
    }

    pub fn finish(&self, turn: TurnId) {
        self.active.lock().unwrap().remove(&turn);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupts_only_model_phase() {
        let controls = Controls::default();
        let turn = TurnId::new();
        let stop = CancellationToken::new();
        controls.register(turn, SessionId::new(), stop.clone());
        let handoff = || Ok(true);
        assert!(!controls.interrupt_model(turn, handoff).unwrap());
        controls.model_started(turn);
        assert!(!controls.interrupt_model(turn, || Ok(false)).unwrap());
        assert!(controls.model_settled(turn));
        assert!(!controls.interrupt_model(turn, handoff).unwrap());
        assert!(!stop.is_cancelled());
        controls.model_started(turn);
        assert!(controls.interrupt_model(turn, handoff).unwrap());
        assert!(stop.is_cancelled() && controls.yielded(turn));
        // Tools from an interrupted response never start.
        assert!(!controls.model_settled(turn));
        controls.cancel(turn);
        assert!(controls.model_settled(turn));
    }

    #[tokio::test]
    async fn streams_only_active_owned_runs() {
        let controls = Controls::default();
        let session = SessionId::new();
        let turn = TurnId::new();
        let calls = std::cell::Cell::new(0);
        let publish = || {
            calls.set(calls.get() + 1);
            Ok(())
        };
        controls.register(turn, session, CancellationToken::new());
        controls.stream(turn, session, publish).unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(
            controls
                .stream(turn, SessionId::new(), publish)
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        controls.cancel(turn);
        controls.stream(turn, session, publish).unwrap();
        controls.finish(turn);
        controls.stream(turn, session, publish).unwrap();

        let draining = TurnId::new();
        controls.register(draining, session, CancellationToken::new());
        controls.drain(draining).await;
        controls.stream(draining, session, publish).unwrap();

        let shutdown = TurnId::new();
        controls.register(shutdown, session, CancellationToken::new());
        controls.stop();
        controls.stream(shutdown, session, publish).unwrap();
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn serializes_publication_with_cancellation() {
        use std::{sync::mpsc, time::Duration};

        let controls = Arc::new(Controls::default());
        let turn = TurnId::new();
        let session = SessionId::new();
        let stop = CancellationToken::new();
        controls.register(turn, session, stop.clone());
        let (entered, entering) = mpsc::channel();
        let (release, releasing) = mpsc::channel();
        let producer = controls.clone();
        let publishing = std::thread::spawn(move || {
            producer
                .stream(turn, session, || {
                    entered.send(()).unwrap();
                    releasing.recv_timeout(Duration::from_secs(2)).unwrap();
                    Ok(())
                })
                .unwrap();
        });
        entering.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(controls.active.try_lock().is_err());

        let (attempted, attempting) = mpsc::channel();
        let (finished, finishing) = mpsc::channel();
        let cancelling = controls.clone();
        let cancellation = std::thread::spawn(move || {
            attempted.send(()).unwrap();
            cancelling.cancel(turn);
            finished.send(()).unwrap();
        });
        attempting.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            finishing.recv_timeout(Duration::from_millis(20)),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        assert!(!stop.is_cancelled());
        release.send(()).unwrap();
        publishing.join().unwrap();
        finishing.recv_timeout(Duration::from_secs(2)).unwrap();
        cancellation.join().unwrap();
        assert!(stop.is_cancelled());
        controls
            .stream(turn, session, || panic!("cancelled stream was published"))
            .unwrap();
    }

    #[test]
    fn reuses_scoped_scratch_until_last_lease() {
        let profile = tempfile::tempdir().unwrap();
        let controls = Controls::default();
        let worktree = WorktreeId::new();
        let session = SessionId::new();
        let turn = TurnId::new();
        controls.register(turn, session, CancellationToken::new());
        let first = controls
            .scratch(turn, profile.path(), worktree, session)
            .unwrap();
        let second = controls
            .scratch(turn, profile.path(), worktree, session)
            .unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        let path = first.path().to_owned();
        assert_eq!(path.parent(), Some(profile.path().join("tmp").as_path()));
        assert!(
            controls
                .scratch(turn, profile.path(), worktree, SessionId::new())
                .is_err()
        );
        let other = TurnId::new();
        controls.register(other, session, CancellationToken::new());
        let separate = controls
            .scratch(other, profile.path(), worktree, session)
            .unwrap();
        assert_ne!(separate.path(), path);
        controls.finish(turn);
        assert!(
            controls
                .scratch(turn, profile.path(), worktree, session)
                .is_err()
        );
        drop(first);
        assert!(path.exists());
        drop(second);
        assert!(!path.exists());
        let other_path = separate.path().to_owned();
        drop(separate);
        assert!(other_path.exists());
        controls.finish(other);
        assert!(!other_path.exists());
    }

    #[tokio::test]
    async fn seals_operations_before_completion() {
        let controls = Controls::default();
        let turn = TurnId::new();
        controls.register(turn, SessionId::new(), CancellationToken::new());
        let token = controls.begin_operation(turn).unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), controls.drain(turn))
                .await
                .is_err()
        );
        assert!(controls.begin_operation(turn).is_none());
        assert!(!token.is_cancelled());
        controls.finish_operation(turn);
        controls.drain(turn).await;
        controls.finish(turn);
        assert!(controls.begin_operation(turn).is_none());
    }

    #[test]
    fn older_cleanup_retains_reused_child() {
        let controls = Controls::default();
        let parent = SessionId::new();
        let child = SessionId::new();
        let previous = TurnId::new();
        let current = TurnId::new();
        let stop = CancellationToken::new();
        controls.register_child(parent, child, previous, CancellationToken::new());
        controls.register_child(parent, child, current, stop.clone());
        controls.finish_child(parent, child, previous);
        assert!(controls.has_children(parent));
        controls.cancel_children(parent);
        assert!(stop.is_cancelled());
        controls.finish_child(parent, child, current);
        assert!(!controls.has_children(parent));
    }

    #[test]
    fn guidance_preserves_owned_children() {
        let controls = Controls::default();
        let parent = SessionId::new();
        let previous = TurnId::new();
        let current = TurnId::new();
        let child = CancellationToken::new();
        controls.register(previous, parent, CancellationToken::new());
        controls.register_child(parent, SessionId::new(), TurnId::new(), child.clone());
        controls.yield_run(previous);
        assert!(controls.yielded(previous));
        assert!(!child.is_cancelled());
        controls.finish(previous);
        controls.register(current, parent, CancellationToken::new());
        controls.cancel(current);
        assert!(!controls.yielded(current));
        assert!(child.is_cancelled());
    }

    #[test]
    fn stop_overrides_handoff() {
        for shutdown in [false, true] {
            let controls = Controls::default();
            let turn = TurnId::new();
            let token = CancellationToken::new();
            controls.register(turn, SessionId::new(), token.clone());
            controls.yield_run(turn);
            assert!(controls.yielded(turn));
            assert!(token.is_cancelled());
            if shutdown {
                controls.stop();
            } else {
                controls.cancel(turn);
            }
            assert!(!controls.yielded(turn));
            controls.yield_run(turn);
            assert!(!controls.yielded(turn));
        }
    }
}
