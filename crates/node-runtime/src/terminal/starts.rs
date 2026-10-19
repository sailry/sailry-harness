//! Startup reservations share the terminal inventory without retaining its lock across I/O.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct State {
    pub(super) handles: BTreeMap<TerminalId, Arc<session::Handle>>,
    pending: BTreeSet<TerminalId>,
}

pub(super) enum Start<'a> {
    Existing(Arc<session::Handle>),
    Reserved(Reservation<'a>),
}

pub(super) struct Reservation<'a> {
    owner: &'a Terminals,
    pub(super) id: TerminalId,
    pub(super) revision: u64,
}

impl Terminals {
    pub(super) fn reserve(&self, previous: Option<&Info>) -> Result<Start<'_>, Fault> {
        let id = previous.map_or_else(TerminalId::new, |info| info.id);
        let mut state = self.sessions.lock().unwrap();
        loop {
            if self.stopping.is_cancelled() {
                return Err(unavailable());
            }
            if previous.is_some()
                && let Some(handle) = state.handles.get(&id)
            {
                return Ok(Start::Existing(handle.clone()));
            }
            if !state.pending.contains(&id) {
                break;
            }
            state = self.started.wait(state).unwrap();
        }
        if state
            .handles
            .values()
            .filter(|handle| handle.running())
            .count()
            + state.pending.len()
            >= 32
        {
            return Err(Fault::new(ErrorCode::Busy, "terminal capacity exhausted"));
        }
        state.pending.insert(id);
        Ok(Start::Reserved(Reservation {
            owner: self,
            id,
            revision: previous.map_or(1, |info| info.revision + 1),
        }))
    }
}

impl Reservation<'_> {
    pub(super) fn publish(self, handle: Arc<session::Handle>) -> Result<Info, Fault> {
        let mut state = self.owner.sessions.lock().unwrap();
        if self.owner.stopping.is_cancelled() {
            drop(state);
            handle.close();
            return Err(unavailable());
        }
        let info = handle.snapshot.lock().unwrap().info.clone();
        state.pending.remove(&self.id);
        state.handles.insert(self.id, handle);
        drop(state);
        Ok(info)
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        self.owner.sessions.lock().unwrap().pending.remove(&self.id);
        self.owner.started.notify_all();
    }
}

impl State {
    pub(super) fn pending(&self) -> bool {
        !self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_releases_reserved_capacity() {
        let terminals = Terminals::new(NodeId([71; 32]), |_| {});
        let mut reserved = (0..32)
            .map(|_| terminals.reserve(None).unwrap())
            .collect::<Vec<_>>();
        assert!(matches!(terminals.reserve(None), Err(error) if error.code == ErrorCode::Busy));
        reserved.pop();
        assert!(matches!(
            terminals.reserve(None).unwrap(),
            Start::Reserved(_)
        ));
        drop(reserved);
        assert!(!terminals.sessions.lock().unwrap().pending());
        terminals.stop();
        assert!(
            matches!(terminals.reserve(None), Err(error) if error.code == ErrorCode::Unavailable)
        );
    }

    #[test]
    fn waits_for_same_id_without_consuming_capacity() {
        let terminals = Terminals::new(NodeId([72; 32]), |_| {});
        let previous = Info {
            id: TerminalId::new(),
            worktree: None,
            ssh: Some(SshId::new()),
            tool: None,
            access: None,
            status: Status::Stopped,
            activity: None,
            title: None,
            directory: None,
            owner: None,
            revision: 4,
        };
        for stopping in [false, true] {
            let reservation = terminals.reserve(Some(&previous)).unwrap();
            std::thread::scope(|scope| {
                let (entered, ready) = std::sync::mpsc::channel();
                let owner = terminals.clone();
                let previous = &previous;
                let waiting = scope.spawn(move || {
                    entered.send(()).unwrap();
                    owner.reserve(Some(previous)).map(|start| {
                        let Start::Reserved(reserved) = start else {
                            panic!("failed starts must release their reservation")
                        };
                        assert_eq!(reserved.id, previous.id);
                        assert_eq!(reserved.revision, 5);
                    })
                });
                ready.recv().unwrap();
                assert_eq!(terminals.sessions.lock().unwrap().pending.len(), 1);
                if stopping {
                    terminals.stop();
                }
                drop(reservation);
                let result = waiting.join().unwrap();
                if stopping {
                    assert!(matches!(result, Err(error) if error.code == ErrorCode::Unavailable));
                } else {
                    result.unwrap();
                }
            });
            assert!(!terminals.sessions.lock().unwrap().pending());
        }
    }
}
