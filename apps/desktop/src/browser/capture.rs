//! Wry 0.53.3 has no portable capture API; macOS delegates to its native WKWebView.
use super::*;
use sailry_protocol::{ErrorCode, Fault, browser};

impl Browser {
    #[cfg(target_os = "macos")]
    pub(super) fn screenshot(
        &self,
        tab: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<browser::Result> {
        use objc2::MainThreadOnly;
        use objc2_app_kit::NSImage;
        use objc2_foundation::{NSError, NSNumber};
        use objc2_web_kit::WKSnapshotConfiguration;
        use std::{cell::RefCell, time::Duration};
        use wry::WebViewExtMacOS;
        let Some(page) = self
            .tabs
            .iter()
            .find(|value| value.id as u64 == tab)
            .and_then(|tab| tab.page.as_ref())
        else {
            return Task::ready(Err(failed()));
        };
        let view = page.read(cx).raw().webview();
        let (send, receive) = tokio::sync::oneshot::channel();
        let send = RefCell::new(Some(send));
        let callback = block2::RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
            let result = if !error.is_null() {
                Err(failed())
            } else {
                unsafe { image.as_ref() }
                    .and_then(NSImage::TIFFRepresentation)
                    .filter(|data| data.len() <= 32 * 1024 * 1024)
                    .map(|data| data.to_vec())
                    .ok_or_else(failed)
            };
            if let Some(send) = send.borrow_mut().take() {
                let _ = send.send(result);
            }
        });
        unsafe {
            let config = WKSnapshotConfiguration::new(view.mtm());
            config.setSnapshotWidth(Some(&NSNumber::new_f64(960.)));
            config.setAfterScreenUpdates(false);
            view.takeSnapshotWithConfiguration_completionHandler(Some(&config), &callback);
        }
        cx.spawn_in(window, async move |_, cx| {
            let timer = cx.background_executor().timer(Duration::from_secs(5));
            let data = tokio::select! {
                result = receive => result.map_err(|_| failed())??,
                _ = timer => return Err(failed()),
            };
            cx.background_executor()
                .spawn(async move { encode(tab, &data) })
                .await
        })
    }

    #[cfg(not(target_os = "macos"))]
    pub(super) fn screenshot(
        &self,
        _tab: u64,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Task<browser::Result> {
        Task::ready(Err(Fault::new(
            ErrorCode::Unavailable,
            "embedded browser capture is unavailable on this platform; use external browser automation",
        )))
    }
}

#[cfg(target_os = "macos")]
fn failed() -> Fault {
    Fault::new(ErrorCode::Unavailable, "browser capture failed")
}

#[cfg(target_os = "macos")]
fn encode(tab: u64, bytes: &[u8]) -> browser::Result {
    use base64::Engine;
    let mut image = image::load_from_memory_with_format(bytes, image::ImageFormat::Tiff)
        .map_err(|_| failed())?;
    image = image.thumbnail(960, 960);
    // Leave room for the JSON envelope within the existing browser result limit.
    for size in [960, 720, 480, 320] {
        image = image.thumbnail(size, size);
        let mut data = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut data, 65)
            .encode_image(&image.to_rgb8())
            .map_err(|_| failed())?;
        if data.len() <= 180 * 1024 {
            return Ok(
                serde_json::json!({"tab":tab,"mime_type":"image/jpeg","width":image.width(),"height":image.height(),"base64_image":base64::engine::general_purpose::STANDARD.encode(data)}),
            );
        }
    }
    Err(failed())
}
