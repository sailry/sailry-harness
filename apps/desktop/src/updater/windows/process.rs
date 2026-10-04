//! Narrow Windows adapters for ACLs, an exact process lifetime, and OS version.

use std::{io, os::windows::ffi::OsStrExt, path::Path, time::Duration};
use windows_sys::{
    Wdk::System::SystemServices::RtlGetVersion,
    Win32::{
        Foundation::{
            CloseHandle, FILETIME, HANDLE, LocalFree, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
            DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, SetFileSecurityW,
        },
        System::{
            SystemInformation::OSVERSIONINFOW,
            Threading::{
                GetCurrentProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
                PROCESS_SYNCHRONIZE, WaitForSingleObject,
            },
        },
    },
};

pub(super) fn system_version() -> Result<String, String> {
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // RtlGetVersion reports the actual OS build, unaffected by manifest-based
    // GetVersionEx compatibility virtualization (Windows 11 also reports 10.0).
    let status = unsafe { RtlGetVersion(&mut version) };
    if status < 0 {
        return Err(format!(
            "cannot determine the Windows version: NTSTATUS {status:#x}"
        ));
    }
    Ok(format!(
        "{}.{}.{}",
        version.dwMajorVersion, version.dwMinorVersion, version.dwBuildNumber
    ))
}

pub(super) fn protect(path: &Path) -> Result<(), String> {
    // OWNER RIGHTS is evaluated against each object's owner. Children inherit
    // the same protected owner/System permissions; no credential is accessed.
    let descriptor: Vec<u16> = "D:P(A;OICI;FA;;;OW)(A;OICI;FA;;;SY)"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut security = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            descriptor.as_ptr(),
            SDDL_REVISION_1,
            &mut security,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(format!(
            "cannot create private updater permissions: {}",
            io::Error::last_os_error()
        ));
    }
    let result = unsafe {
        SetFileSecurityW(
            path.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            security,
        )
    };
    let error = (result == 0).then(io::Error::last_os_error);
    unsafe { LocalFree(security) };
    if let Some(error) = error {
        return Err(format!("cannot protect the updater workspace: {error}"));
    }
    Ok(())
}

pub(super) fn current_started() -> Result<u64, String> {
    // The pseudo handle is owned by Windows and must not be closed.
    started(unsafe { GetCurrentProcess() })
}

fn started(handle: HANDLE) -> Result<u64, String> {
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    if unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
        return Err(format!(
            "cannot inspect the original process: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(u64::from(created.dwHighDateTime) << 32 | u64::from(created.dwLowDateTime))
}

pub(super) struct Parent(HANDLE);

impl Parent {
    pub(super) fn open(pid: u32, expected_started: u64) -> Result<Self, String> {
        let handle = unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            )
        };
        if handle.is_null() {
            return Err(format!(
                "cannot open the original Sailry process: {}",
                io::Error::last_os_error()
            ));
        }
        let parent = Self(handle);
        if started(handle)? != expected_started {
            return Err("the original Sailry process identity has changed".into());
        }
        if unsafe { WaitForSingleObject(handle, 0) } != WAIT_TIMEOUT {
            return Err("the original Sailry process is no longer running".into());
        }
        Ok(parent)
    }

    pub(super) fn wait(&self, timeout: Duration) -> Result<(), String> {
        let millis =
            u32::try_from(timeout.as_millis()).map_err(|_| "invalid update wait timeout")?;
        match unsafe { WaitForSingleObject(self.0, millis) } {
            WAIT_OBJECT_0 => Ok(()),
            WAIT_TIMEOUT => Err("Sailry did not exit; the installed bundle was not changed".into()),
            WAIT_FAILED => Err(format!(
                "cannot wait for Sailry to exit: {}",
                io::Error::last_os_error()
            )),
            _ => Err("unexpected process wait result; the installed bundle was not changed".into()),
        }
    }
}

impl Drop for Parent {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}
