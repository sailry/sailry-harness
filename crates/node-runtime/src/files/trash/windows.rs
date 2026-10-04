//! trash-rs 5.2.6 exposes no Windows operation flags and uses ALLOWUNDO with
//! WANTNUKEWARNING, which can offer permanent deletion. This small native adapter
//! requires recycling and suppresses all UI for the headless mutation worker.
//! Contract: IFileOperation::SetOperationFlags (Microsoft Learn).
use super::{Fault, Path, unknown};
use ::windows::{
    Win32::{System::Com::*, UI::Shell::*},
    core::HSTRING,
};

pub(super) fn trash(target: &Path) -> Result<(), Fault> {
    recycle(target).map_err(|_| unknown())
}

fn recycle(target: &Path) -> ::windows::core::Result<()> {
    // SAFETY: initialize and release COM on this same bounded worker thread.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).ok()? };
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            // SAFETY: construction follows successful CoInitializeEx, including
            // S_FALSE. Interfaces below are dropped before the apartment guard.
            unsafe { CoUninitialize() };
        }
    }
    let _apartment = Apartment;
    let path = HSTRING::from(target.as_os_str());
    // SAFETY: COM is initialized; the string and interface references stay
    // alive through the synchronous operation. No shell item escapes the call.
    unsafe {
        let operation: IFileOperation =
            CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)?;
        operation.SetOperationFlags(
            FOF_NO_UI | FOF_NO_CONNECTED_ELEMENTS | FOFX_RECYCLEONDELETE | FOFX_EARLYFAILURE,
        )?;
        let item: IShellItem = SHCreateItemFromParsingName(&path, None)?;
        operation.DeleteItem(&item, None)?;
        operation.PerformOperations()?;
        if operation.GetAnyOperationsAborted()?.as_bool() {
            return Err(::windows::core::Error::from_hresult(
                ::windows::Win32::Foundation::E_ABORT,
            ));
        }
    }
    Ok(())
}
