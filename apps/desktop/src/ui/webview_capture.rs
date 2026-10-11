//! Wry 0.53.3 has no portable capture API. Share its retained native platform
//! adapters between browser automation and inert preview placeholders.
use tokio::sync::oneshot;

pub(crate) struct Buffer {
    pub(crate) bytes: Vec<u8>,
    pub(crate) format: image::ImageFormat,
}

type Pending = oneshot::Receiver<Result<Buffer, ()>>;

#[cfg(target_os = "macos")]
pub(crate) fn capture(page: &wry::WebView, width: Option<f64>) -> Result<Pending, ()> {
    use objc2::MainThreadOnly;
    use objc2_app_kit::NSImage;
    use objc2_foundation::{NSError, NSNumber};
    use objc2_web_kit::WKSnapshotConfiguration;
    use std::cell::RefCell;
    use wry::WebViewExtMacOS;
    let view = page.webview();
    let (send, receive) = tokio::sync::oneshot::channel();
    let send = RefCell::new(Some(send));
    let callback = block2::RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
        let result = if !error.is_null() {
            Err(())
        } else {
            // SAFETY: WebKit supplies an optional NSImage valid on the main thread
            // for this callback; as_ref handles nil and the bytes are copied before return.
            unsafe { image.as_ref() }
                .and_then(NSImage::TIFFRepresentation)
                .filter(|data| data.len() <= 32 * 1024 * 1024)
                .map(|data| Buffer {
                    bytes: data.to_vec(),
                    format: image::ImageFormat::Tiff,
                })
                .ok_or(())
        };
        if let Some(send) = send.borrow_mut().take() {
            let _ = send.send(result);
        }
    });
    // SAFETY: view is retained on GPUI's main thread, snapshot arguments have
    // the declared types, and WebKit copies the block for its main-thread completion.
    unsafe {
        let config = WKSnapshotConfiguration::new(view.mtm());
        config.setSnapshotWidth(width.map(NSNumber::new_f64).as_deref());
        config.setAfterScreenUpdates(false);
        view.takeSnapshotWithConfiguration_completionHandler(Some(&config), &callback);
    }
    Ok(receive)
}

#[cfg(target_os = "windows")]
pub(crate) fn capture(page: &wry::WebView, _width: Option<f64>) -> Result<Pending, ()> {
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
    // SAFETY: COM is initialized by Wry on this UI thread. A null HGLOBAL
    // requests an owned stream which frees its backing allocation on release.
    let stream = match unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true) } {
        Ok(stream) => stream,
        Err(_) => return Err(()),
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
            Ok(Buffer {
                bytes: data,
                format: image::ImageFormat::Png,
            })
        });
        if let Some(sender) = sender.borrow_mut().take() {
            let _ = sender.send(result.map_err(|_| ()));
        }
        Ok(())
    }));
    // SAFETY: Wry retains the WebView2 interface on this UI thread; WebView2
    // retains the stream and completion handler throughout CapturePreview.
    if unsafe {
        page.webview().CapturePreview(
            COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
            &stream,
            &handler,
        )
    }
    .is_err()
    {
        return Err(());
    }
    Ok(receive)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn capture(_page: &wry::WebView, _width: Option<f64>) -> Result<Pending, ()> {
    Err(())
}
