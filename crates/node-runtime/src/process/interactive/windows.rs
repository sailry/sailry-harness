use super::failure;
use sailry_protocol::Fault;
use std::{
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    sync::Arc,
};
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject,
};

pub(super) struct Job(Arc<OwnedHandle>);

impl Job {
    pub(super) fn new() -> Result<Self, Fault> {
        // SAFETY: null attributes and name create a private, non-inheritable Job.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(failure(std::io::Error::last_os_error()));
        }
        // SAFETY: CreateJobObjectW returned a new owned handle; no other owner closes it.
        let job = Self(Arc::new(unsafe { OwnedHandle::from_raw_handle(handle) }));
        let mut information = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: the private Job handle is valid. The pointer
        // references a correctly sized information struct for this call only.
        let configured = unsafe {
            SetInformationJobObject(
                job.0.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                &information as *const _ as _,
                std::mem::size_of_val(&information) as u32,
            )
        };
        if configured == 0 {
            return Err(failure(std::io::Error::last_os_error()));
        }
        Ok(job)
    }

    pub(super) fn configure(&self, command: &mut portable_pty::CommandBuilder) {
        command.set_windows_job(self.0.clone());
    }
}

#[cfg(test)]
mod tests;
