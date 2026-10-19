//! A retained PTY leader anchors cleanup of its own session, including background job groups.
use sailry_protocol::{ErrorCode, Fault};
use sysinfo::{Pid, Process, ProcessRefreshKind, ProcessesToUpdate, System, Uid, UpdateKind};

pub(super) struct Owner {
    root: Pid,
    started: u64,
    user: Uid,
}

fn refresh(system: &mut System, selection: ProcessesToUpdate<'_>) {
    system.refresh_processes_specifics(
        selection,
        true,
        ProcessRefreshKind::nothing().with_user(UpdateKind::Always),
    );
}

impl Owner {
    pub(super) fn capture(child: &dyn portable_pty::Child) -> Option<Self> {
        let root = Pid::from_u32(child.process_id()?);
        if root.as_u32() <= 1 || root.as_u32() == std::process::id() {
            return None;
        }
        let mut system = System::new();
        refresh(&mut system, ProcessesToUpdate::Some(&[root]));
        let process = system.process(root)?;
        if process.session_id() != Some(root) || process.start_time() == 0 {
            return None;
        }
        Some(Self {
            root,
            started: process.start_time(),
            user: process.user_id()?.clone(),
        })
    }

    fn leader(&self, system: &System) -> bool {
        system
            .process(self.root)
            .is_some_and(|process| self.member(process) && process.start_time() == self.started)
    }

    fn member(&self, process: &Process) -> bool {
        process.session_id() == Some(self.root)
            && process.user_id() == Some(&self.user)
            && process.start_time() >= self.started
    }

    pub(super) fn stop(&self) -> Result<(), Fault> {
        let mut system = System::new();
        refresh(&mut system, ProcessesToUpdate::All);
        if !self.leader(&system) {
            return Err(changed());
        }
        let members = system
            .processes()
            .iter()
            .filter_map(|(&pid, process)| {
                (pid != self.root && self.member(process)).then_some((pid, process.start_time()))
            })
            .collect::<Vec<_>>();
        // Keep the exact leader alive as the session anchor until its members are handled.
        for (pid, started) in members {
            refresh(&mut system, ProcessesToUpdate::Some(&[self.root, pid]));
            if !self.leader(&system) {
                return Err(changed());
            }
            let Some(process) = system.process(pid) else {
                continue;
            };
            if !self.member(process) || process.start_time() != started {
                continue;
            }
            match crate::host::processes::stop(pid.as_u32(), started, true) {
                Ok(()) => {}
                Err(error) if matches!(error.code, ErrorCode::NotFound | ErrorCode::Conflict) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

fn changed() -> Fault {
    Fault::new(
        ErrorCode::Conflict,
        "terminal process ownership is no longer verifiable",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_changed_leader_without_signalling() {
        let root = Pid::from_u32(std::process::id());
        let mut system = System::new();
        refresh(&mut system, ProcessesToUpdate::Some(&[root]));
        let current = system.process(root).unwrap();
        let owner = Owner {
            root,
            started: current.start_time() + 1,
            user: current.user_id().unwrap().clone(),
        };
        assert!(matches!(owner.stop(), Err(error) if error.code == ErrorCode::Conflict));
    }
}
