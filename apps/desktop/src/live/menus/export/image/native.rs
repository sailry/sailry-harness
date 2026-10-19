use super::*;
use raw_window_handle::HasWindowHandle;
use std::{borrow::Cow, collections::BTreeMap, time::Duration};

const URL: &str = "sailry-export://localhost/index.html";
const WIDTH: u32 = 960;
const MAX_HEIGHT: u32 = 32768;

pub(super) fn render(
    html: String,
    assets: String,
    directory: PathBuf,
    static_html: bool,
    window: &mut Window,
    cx: &mut App,
) -> Task<Result<Vec<u8>, &'static str>> {
    let result = create(html, assets, directory, static_html, window);
    let Ok((view, mut receiver)) = result else {
        return Task::ready(Err("conversation_export_failed"));
    };
    // Keep the native child on the main thread until its job completes or is dropped.
    window.spawn(cx, async move |cx| {
        let _view = view;
        let timer = cx.background_executor().timer(Duration::from_secs(90));
        let value = tokio::select! { message = receiver.recv() => message.ok_or("conversation_export_failed")?, _ = timer => return Err("conversation_export_failed") };
        cx.background_executor().spawn(async move { decode(value, static_html) }).await
    })
}

fn create(
    html: String,
    assets: String,
    directory: PathBuf,
    static_html: bool,
    window: &mut Window,
) -> Result<(wry::WebView, tokio::sync::mpsc::Receiver<String>), String> {
    let mut files = BTreeMap::new();
    for entry in std::fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        files.insert(
            format!("/{assets}/{}", entry.file_name().to_string_lossy()),
            entry.path(),
        );
    }
    let (body, end) = html.rsplit_once("</body>").ok_or("Missing document body")?;
    let capture = if static_html {
        format!("<script src=\"{assets}/static.js\"></script>")
    } else {
        format!(
            "<script src=\"{assets}/html2canvas.js\"></script><script src=\"{assets}/capture.js\"></script>"
        )
    };
    let html = format!("{body}{capture}</body>{end}");
    let (send, receive) = tokio::sync::mpsc::channel(1);
    let handle = HasWindowHandle::window_handle(window).map_err(|error| error.to_string())?;
    let view = wry::WebViewBuilder::new()
        .with_custom_protocol("sailry-export".into(), move |_, request| {
            let path = request.uri().path();
            let body =
                if request.method() != wry::http::Method::GET || request.uri().query().is_some() {
                    None
                } else if path == "/index.html" {
                    Some(html.as_bytes().to_vec())
                } else {
                    files
                        .get(path)
                        .and_then(|path| std::fs::read(path).ok())
                        .filter(|bytes| bytes.len() <= 64 * 1024 * 1024)
                };
            let kind = if path == "/index.html" {
                "text/html; charset=utf-8"
            } else if path.ends_with(".js") {
                "text/javascript; charset=utf-8"
            } else {
                mime_guess::from_path(path)
                    .first_raw()
                    .unwrap_or("application/octet-stream")
            };
            wry::http::Response::builder()
                .status(if body.is_some() { 200 } else { 404 })
                .header("Content-Type", kind)
                .header("X-Content-Type-Options", "nosniff")
                .body(Cow::Owned(body.unwrap_or_default()))
                .expect("Static response headers")
        })
        .with_url(URL)
        .with_incognito(true)
        .with_visible(false)
        .with_background_throttling(wry::BackgroundThrottlingPolicy::Disabled)
        .with_initialization_script("addEventListener('error', event => window.ipc.postMessage(JSON.stringify({error:String(event.message || 'Export asset failed')}))); addEventListener('unhandledrejection', event => window.ipc.postMessage(JSON.stringify({error:String(event.reason)})));")
        .with_bounds(wry::Rect {
            position: wry::dpi::LogicalPosition::new(0., 0.).into(),
            size: wry::dpi::LogicalSize::new(f64::from(WIDTH), 768.).into(),
        })
        .with_navigation_handler(|url| {
            url == URL || url == "http://sailry-export.localhost/index.html" || url == "about:blank"
        })
        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
        .with_download_started_handler(|_, _| false)
        .with_ipc_handler(move |request| {
            if request.body().len() <= 64 * 1024 * 1024 {
                let _ = send.try_send(request.body().clone());
            }
        })
        .build_as_child(&handle)
        .map_err(|error| error.to_string())?;
    Ok((view, receive))
}

fn decode(value: String, static_html: bool) -> Result<Vec<u8>, &'static str> {
    if static_html {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Document {
            html: String,
        }
        let document: Document =
            serde_json::from_str(&value).map_err(|_| "conversation_export_failed")?;
        if document.html.len() > 64 * 1024 * 1024 || !document.html.starts_with("<!doctype html>") {
            return Err("conversation_export_failed");
        }
        return Ok(document.html.into_bytes());
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Tiles {
        height: u32,
        tiles: Vec<String>,
    }
    let value: Tiles = serde_json::from_str(&value).map_err(|_| "conversation_export_failed")?;
    if value.height == 0
        || value.height > MAX_HEIGHT
        || value.tiles.len() != value.height.div_ceil(2048) as usize
    {
        return Err("conversation_export_failed");
    }
    let mut output = image::RgbaImage::new(WIDTH, value.height);
    let mut y = 0;
    for tile in value.tiles {
        let data = base64::engine::general_purpose::STANDARD
            .decode(
                tile.strip_prefix("data:image/png;base64,")
                    .ok_or("conversation_export_failed")?,
            )
            .map_err(|_| "conversation_export_failed")?;
        let tile = image::load_from_memory_with_format(&data, image::ImageFormat::Png)
            .map_err(|_| "conversation_export_failed")?
            .into_rgba8();
        if tile.width() != WIDTH || tile.height() != (value.height - y).min(2048) {
            return Err("conversation_export_failed");
        }
        image::imageops::replace(&mut output, &tile, 0, i64::from(y));
        y += tile.height();
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(output)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|_| "conversation_export_failed")?;
    Ok(bytes.into_inner())
}
