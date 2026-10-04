//! CoreAudio can return silent buffers until macOS microphone access is granted.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Status {
    Unknown,
    NotDetermined,
    Granted,
    Denied,
    Restricted,
}

pub(crate) fn status() -> Status {
    #[cfg(target_os = "macos")]
    {
        use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
        let Some(media) = (unsafe { AVMediaTypeAudio }) else {
            return Status::Unknown;
        };
        match unsafe { AVCaptureDevice::authorizationStatusForMediaType(media) } {
            AVAuthorizationStatus::Authorized => Status::Granted,
            AVAuthorizationStatus::NotDetermined => Status::NotDetermined,
            AVAuthorizationStatus::Denied => Status::Denied,
            AVAuthorizationStatus::Restricted => Status::Restricted,
            _ => Status::Unknown,
        }
    }
    #[cfg(not(target_os = "macos"))]
    Status::Unknown
}

pub(crate) fn settings_url() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
    } else if cfg!(target_os = "windows") {
        Some("ms-settings:privacy-microphone")
    } else {
        None
    }
}

pub(crate) async fn ensure(cancel: &CancellationToken) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
        let receiver = {
            // AVMediaTypeAudio is the supported media type for microphone authorization.
            let media = unsafe { AVMediaTypeAudio }.ok_or("dictation_unavailable")?;
            let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(media) };
            if status == AVAuthorizationStatus::Authorized {
                return Ok(());
            }
            if status != AVAuthorizationStatus::NotDetermined {
                return Err("dictation_permission");
            }
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let sender = std::sync::Mutex::new(Some(sender));
            let callback = block2::RcBlock::new(move |granted: objc2::runtime::Bool| {
                if let Some(sender) = sender.lock().unwrap().take() {
                    let _ = sender.send(granted.as_bool());
                }
            });
            // AVFoundation copies the completion block and calls it on its own queue.
            unsafe {
                AVCaptureDevice::requestAccessForMediaType_completionHandler(media, &callback);
            }
            receiver
        };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err("dictation_cancelled"),
            result = receiver => if matches!(result, Ok(true)) { Ok(()) } else { Err("dictation_permission") },
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = cancel;
        Ok(())
    }
}
