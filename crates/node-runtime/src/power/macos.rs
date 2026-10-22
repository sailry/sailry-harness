use objc2_core_foundation::CFString;
use objc2_io_kit::{IOPMAssertionCreateWithName, IOPMAssertionRelease, kIOPMAssertionLevelOn};

#[derive(Default)]
pub(super) struct Native(Option<u32>);

impl super::Backend for Native {
    fn held(&self) -> bool {
        self.0.is_some()
    }
    fn acquire(&mut self) -> Result<(), i64> {
        // IOPM's public constant is a CFSTR macro, absent from generated bindings.
        let kind = CFString::from_str("PreventUserIdleSystemSleep");
        let reason = CFString::from_str("Sailry task execution");
        let mut id = 0;
        // SAFETY: Both borrowed strings and the writable ID outlive this call.
        let result = unsafe {
            IOPMAssertionCreateWithName(Some(&kind), kIOPMAssertionLevelOn, Some(&reason), &mut id)
        };
        if result != 0 {
            return Err(i64::from(result));
        }
        self.0 = Some(id);
        Ok(())
    }

    fn release(&mut self) -> Result<(), i64> {
        if let Some(id) = self.0 {
            let result = IOPMAssertionRelease(id);
            if result != 0 {
                return Err(i64::from(result));
            }
            self.0 = None;
        }
        Ok(())
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        let _ = super::Backend::release(self);
    }
}
