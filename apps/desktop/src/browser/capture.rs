//! Wry 0.53.3 has no portable capture API; use its retained native platform handle.
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
                // SAFETY: WebKit supplies an optional NSImage valid on the main thread
                // for this callback; as_ref handles nil and the bytes are copied before return.
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
        // SAFETY: view is retained on GPUI's main thread, snapshot arguments have
        // the declared types, and WebKit copies the block for its main-thread completion.
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
                .spawn(async move { encode(tab, &data, image::ImageFormat::Tiff) })
                .await
        })
    }

    #[cfg(target_os = "windows")]
    pub(super) fn screenshot(
        &self,
        tab: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<browser::Result> {
        use webview2_com::{
            CapturePreviewCompletedHandler,
            Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
        };
        use windows_webview::Win32::{
            Foundation::HGLOBAL,
            System::Com::{
                STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET, StructuredStorage::CreateStreamOnHGlobal,
            },
        };
        use wry::WebViewExtWindows;
        let Some(page) = self
            .tabs
            .iter()
            .find(|value| value.id as u64 == tab)
            .and_then(|tab| tab.page.as_ref())
        else {
            return Task::ready(Err(failed()));
        };
        // SAFETY: COM is initialized by Wry on this UI thread. A null HGLOBAL
        // requests an owned stream which frees its backing allocation on release.
        let stream = match unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true) } {
            Ok(stream) => stream,
            Err(_) => return Task::ready(Err(failed())),
        };
        let (sender, receive) = tokio::sync::oneshot::channel();
        let sender = std::cell::RefCell::new(Some(sender));
        let output = stream.clone();
        let handler = CapturePreviewCompletedHandler::create(Box::new(move |status| {
            let result = status.and_then(|()| {
                let mut info = STATSTG::default();
                // SAFETY: the completed native capture owns this valid IStream;
                // stat/seek/read use correctly sized buffers on its UI thread.
                unsafe {
                    output.Stat(&mut info, STATFLAG_NONAME)?;
                }
                if info.cbSize == 0 || info.cbSize > 32 * 1024 * 1024 {
                    return Err(windows_webview::core::Error::from_hresult(
                        windows_webview::core::HRESULT(0x80004005u32 as i32),
                    ));
                }
                let mut data = vec![0u8; info.cbSize as usize];
                let mut read = 0;
                // SAFETY: data is allocated for the exact validated size and
                // the count pointer is valid for this synchronous COM call.
                unsafe {
                    output.Seek(0, STREAM_SEEK_SET, None)?;
                    output
                        .Read(data.as_mut_ptr().cast(), data.len() as u32, Some(&mut read))
                        .ok()?;
                }
                if read as usize != data.len() {
                    return Err(windows_webview::core::Error::from_hresult(
                        windows_webview::core::HRESULT(0x80004005u32 as i32),
                    ));
                }
                Ok(data)
            });
            if let Some(sender) = sender.borrow_mut().take() {
                let _ = sender.send(result.map_err(|_| failed()));
            }
            Ok(())
        }));
        // SAFETY: Wry retains the WebView2 interface on this UI thread; WebView2
        // retains the stream and completion handler throughout CapturePreview.
        if unsafe {
            page.read(cx).raw().webview().CapturePreview(
                COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                &stream,
                &handler,
            )
        }
        .is_err()
        {
            return Task::ready(Err(failed()));
        }
        cx.spawn_in(window, async move |_, cx| {
            let timer = cx
                .background_executor()
                .timer(std::time::Duration::from_secs(5));
            let data = tokio::select! {
                result = receive => result.map_err(|_| failed())??,
                _ = timer => return Err(failed()),
            };
            cx.background_executor()
                .spawn(async move { encode(tab, &data, image::ImageFormat::Png) })
                .await
        })
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
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

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn failed() -> Fault {
    Fault::new(ErrorCode::Unavailable, "browser capture failed")
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn encode(tab: u64, bytes: &[u8], format: image::ImageFormat) -> browser::Result {
    use base64::Engine;
    let mut image = image::load_from_memory_with_format(bytes, format).map_err(|_| failed())?;
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
