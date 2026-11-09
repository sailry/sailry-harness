use std::{io, mem::MaybeUninit};

const CAPACITY: usize = 256;

pub(super) struct Group {
    id: libc::pid_t,
    owner: libc::uid_t,
}

impl Group {
    pub(super) fn owned(id: u32) -> Option<Self> {
        let id = libc::pid_t::try_from(id).ok().filter(|id| *id > 1)?;
        Some(Self {
            id,
            // SAFETY: geteuid only reads the calling process's effective identity.
            owner: unsafe { libc::geteuid() },
        })
    }

    pub(super) fn settled(&self) -> bool {
        if self.id <= 1 {
            return false;
        }
        let mut pids = [0 as libc::pid_t; CAPACITY];
        // Darwin's group signal excludes zombies and can return EPERM when only
        // exited members remain. ProcessGroupChild forwards that error unchanged.
        // Verify this exact group rather than ignoring a denied signal to live work.
        // SAFETY: the buffer is writable for its stated size. __error is this
        // thread's errno; libproc returns zero for both empty lists and errors.
        let (written, error) = unsafe {
            *libc::__error() = 0;
            let written = libc::proc_listpgrppids(
                self.id,
                pids.as_mut_ptr().cast(),
                size_of_val(&pids) as libc::c_int,
            );
            (written, *libc::__error())
        };
        let Some(count) = count(written, error) else {
            return false;
        };
        pids[..count]
            .iter()
            .all(|pid| self.exited(*pid, information(*pid)))
    }

    fn exited(&self, pid: libc::pid_t, info: io::Result<libc::proc_bsdinfo>) -> bool {
        let info = match info {
            Ok(info) => info,
            Err(error) => {
                return pid > 1
                    && error.raw_os_error() == Some(libc::ESRCH)
                    && super::group_absent(self.id).is_ok_and(|absent| absent);
            }
        };
        pid > 1
            && info.pbi_pid == pid as u32
            && info.pbi_pgid == self.id as u32
            && info.pbi_uid == self.owner
            && info.pbi_ruid == self.owner
            && info.pbi_svuid == self.owner
            && info.pbi_status == libc::SZOMB
    }
}

fn count(written: libc::c_int, error: libc::c_int) -> Option<usize> {
    let count = usize::try_from(written).ok()?;
    // A full buffer might omit live members. Do not classify an incomplete view.
    (error == 0 && count < CAPACITY).then_some(count)
}

fn information(pid: libc::pid_t) -> io::Result<libc::proc_bsdinfo> {
    if pid <= 1 {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut info = MaybeUninit::<libc::proc_bsdinfo>::uninit();
    // SAFETY: libproc writes at most the exact allocated structure size. Argument
    // 1 includes zombies, which the default argument deliberately does not find.
    let (written, error) = unsafe {
        *libc::__error() = 0;
        let written = libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            1,
            info.as_mut_ptr().cast(),
            size_of::<libc::proc_bsdinfo>() as libc::c_int,
        );
        (written, *libc::__error())
    };
    if written != size_of::<libc::proc_bsdinfo>() as libc::c_int {
        return Err(if written == 0 && error != 0 {
            io::Error::from_raw_os_error(error)
        } else {
            io::ErrorKind::InvalidData.into()
        });
    }
    // SAFETY: only an exact successful full-structure result reaches this point.
    Ok(unsafe { info.assume_init() })
}

#[cfg(test)]
#[path = "macos/tests.rs"]
mod tests;
