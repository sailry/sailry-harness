// Adapted from sailry-code 67ae9fa0, terminal-host/src/shell.rs.
// See third_party_licenses/sailry-code-terminal.md.
use std::{collections::BTreeMap, ffi::OsString};

pub(super) struct Account {
    pub(super) name: OsString,
    pub(super) home: OsString,
    pub(super) shell: OsString,
}

impl Account {
    pub(super) fn apply(&self, values: &mut BTreeMap<OsString, OsString>) {
        for (key, value) in [
            ("HOME", &self.home),
            ("USER", &self.name),
            ("LOGNAME", &self.name),
            ("SHELL", &self.shell),
        ] {
            values.insert(key.into(), value.clone());
        }
    }

    #[cfg(all(unix, not(any(test, feature = "test-support"))))]
    pub(super) fn current() -> std::io::Result<Self> {
        use std::{ffi::CStr, mem::MaybeUninit, os::unix::ffi::OsStringExt, ptr};
        let mut buffer = vec![0_u8; 16 * 1024];
        loop {
            let mut record = MaybeUninit::<libc::passwd>::uninit();
            let mut result = ptr::null_mut();
            // SAFETY: record/result are writable outputs; buffer owns the exact supplied capacity.
            let status = unsafe {
                libc::getpwuid_r(
                    libc::getuid(),
                    record.as_mut_ptr(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                    &mut result,
                )
            };
            if status == libc::ERANGE {
                if buffer.len() >= 1024 * 1024 {
                    return Err(std::io::Error::other("current user account is too large"));
                }
                buffer.resize(buffer.len() * 2, 0);
                continue;
            }
            if status != 0 {
                return Err(std::io::Error::from_raw_os_error(status));
            }
            if result.is_null() {
                return Err(std::io::Error::other("current user account was not found"));
            }
            // SAFETY: successful getpwuid_r initialized record and its strings remain in buffer.
            let record = unsafe { record.assume_init() };
            let read = |value: *const libc::c_char| {
                if value.is_null() {
                    return Err(std::io::Error::other(
                        "current user account has a missing field",
                    ));
                }
                // SAFETY: getpwuid_r provides terminated strings whose buffer is still alive.
                Ok(OsString::from_vec(
                    unsafe { CStr::from_ptr(value) }.to_bytes().to_vec(),
                ))
            };
            return Ok(Self {
                name: read(record.pw_name)?,
                home: read(record.pw_dir)?,
                shell: read(record.pw_shell)?,
            });
        }
    }
}
