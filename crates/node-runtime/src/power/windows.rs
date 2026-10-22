use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE},
    System::{
        Power::{
            PowerClearRequest, PowerCreateRequest, PowerRequestSystemRequired, PowerSetRequest,
        },
        SystemServices::POWER_REQUEST_CONTEXT_VERSION,
        Threading::{POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0},
    },
};

#[derive(Default)]
pub(super) struct Native {
    handle: Option<isize>,
    active: bool,
}

impl super::Backend for Native {
    fn held(&self) -> bool {
        self.active
    }
    fn acquire(&mut self) -> Result<(), i64> {
        let mut reason = "Sailry".encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let context = REASON_CONTEXT {
            Version: POWER_REQUEST_CONTEXT_VERSION,
            Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
            Reason: REASON_CONTEXT_0 {
                SimpleReasonString: reason.as_mut_ptr(),
            },
        };
        // SAFETY: The context and UTF-16 buffer are valid for the call. The
        // returned process handle is owned here, independent of any Tokio thread.
        let handle = unsafe { PowerCreateRequest(&context) };
        if handle == INVALID_HANDLE_VALUE {
            return Err(i64::from(unsafe { GetLastError() }));
        }
        self.handle = Some(handle as isize);
        if unsafe { PowerSetRequest(handle, PowerRequestSystemRequired) } == 0 {
            let code = unsafe { GetLastError() };
            let _ = self.release();
            return Err(i64::from(code));
        }
        self.active = true;
        Ok(())
    }

    fn release(&mut self) -> Result<(), i64> {
        let Some(handle) = self.handle else {
            return Ok(());
        };
        let handle = handle as HANDLE;
        // SAFETY: This process-owned handle remains valid until CloseHandle succeeds.
        let mut failure = None;
        if self.active {
            if unsafe { PowerClearRequest(handle, PowerRequestSystemRequired) } == 0 {
                failure = Some(i64::from(unsafe { GetLastError() }));
            } else {
                self.active = false;
            }
        }
        if unsafe { CloseHandle(handle) } == 0 {
            failure.get_or_insert_with(|| i64::from(unsafe { GetLastError() }));
        } else {
            self.handle = None;
            self.active = false;
        }
        failure.map_or(Ok(()), Err)
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        let _ = super::Backend::release(self);
    }
}
