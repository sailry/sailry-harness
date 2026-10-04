//! Process control runs on the durable mutation worker of the execution Node.
use sailry_protocol::{ErrorCode, Fault};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, Signal, System};

pub(crate) fn stop(pid: u32, started_at_secs: u64, force: bool) -> Result<(), Fault> {
    if pid <= 1 || pid == std::process::id() {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "cannot stop the system or the execution Node",
        ));
    }
    let signal = if force { Signal::Kill } else { Signal::Term };
    if !sysinfo::SUPPORTED_SIGNALS.contains(&signal) {
        return Err(Fault::new(
            ErrorCode::Unavailable,
            "process signal is unsupported on this host",
        ));
    }
    let mut system = System::new();
    let pid = Pid::from_u32(pid);
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    let process = system
        .process(pid)
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "process is no longer running"))?;
    if started_at_secs == 0 || process.start_time() != started_at_secs {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "process identity changed; refresh before retrying",
        ));
    }
    // Revalidate immediately before signalling. OS permissions remain authoritative.
    match process.kill_with(signal) {
        Some(true) => Ok(()),
        Some(false) => Err(Fault::new(
            ErrorCode::Unavailable,
            "could not signal process; it may have exited or require higher privileges",
        )),
        None => Err(Fault::new(
            ErrorCode::Unavailable,
            "process signal is unsupported on this host",
        )),
    }
}
