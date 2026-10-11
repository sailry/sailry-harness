use super::*;
use std::{
    borrow::Cow,
    collections::BTreeMap,
    io::{Cursor, Read},
    sync::OnceLock,
};

type Bundle = BTreeMap<String, Vec<u8>>;
const ARCHIVE: &[u8] = include_bytes!("../web/viewers.zip");
const CSP: &str = "default-src 'none'; script-src 'self' blob: 'wasm-unsafe-eval' 'sha256-IBrtyYCzGTDaWM6ODCra0HbxOzpedBDywXyxe95wzWk='; worker-src blob:; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data: blob:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-src 'none'";

pub(in crate::content::files::html) fn bundle() -> Result<&'static Bundle, &'static str> {
    static BUNDLE: OnceLock<Result<Bundle, &'static str>> = OnceLock::new();
    BUNDLE
        .get_or_init(|| {
            let mut archive = zip::ZipArchive::new(Cursor::new(ARCHIVE))
                .map_err(|_| "artifact_preview_failed")?;
            let mut files = Bundle::new();
            for index in 0..archive.len() {
                let mut file = archive
                    .by_index(index)
                    .map_err(|_| "artifact_preview_failed")?;
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes)
                    .map_err(|_| "artifact_preview_failed")?;
                files.insert(file.name().to_owned(), bytes);
            }
            Ok(files)
        })
        .as_ref()
        .map_err(|key| *key)
}

pub(in crate::content::files::html) fn validate(
    mime: &str,
    bytes: &[u8],
) -> Result<(), &'static str> {
    let format = format(mime).ok_or("artifact_preview_unavailable")?;
    if format == "pdf" {
        return bytes
            .starts_with(b"%PDF-")
            .then_some(())
            .ok_or("artifact_preview_failed");
    }
    // Direct rendering must retain the former Node conversion path's ZIP limits
    // before a browser library expands any file-authored part in memory.
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "artifact_preview_failed")?;
    if archive.len() > 4096 {
        return Err("artifact_preview_large");
    }
    let part = match format {
        "docx" => "word/document.xml",
        "xlsx" => "xl/workbook.xml",
        _ => "ppt/presentation.xml",
    };
    let mut names = std::collections::BTreeSet::new();
    let mut total = 0;
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|_| "artifact_preview_failed")?;
        if !names.insert(file.name().to_owned()) {
            return Err("artifact_preview_failed");
        }
        if file.size() > LIMIT {
            return Err("artifact_preview_large");
        }
        let size = std::io::copy(&mut file.take(LIMIT + 1), &mut std::io::sink())
            .map_err(|_| "artifact_preview_failed")?;
        total += size;
        if size > LIMIT || total > 128 * 1024 * 1024 {
            return Err("artifact_preview_large");
        }
    }
    if !names.contains(part) {
        return Err("artifact_preview_failed");
    }
    package::validate(mime, bytes)
}

fn format(mime: &str) -> Option<&'static str> {
    match mime {
        PDF => Some("pdf"),
        DOCX => Some("docx"),
        XLSX => Some("xlsx"),
        PPTX => Some("pptx"),
        _ => None,
    }
}

fn resource(path: &str) -> Option<(&'static str, Cow<'static, [u8]>)> {
    let bytes: &[u8] = match path {
        "/index.html" => include_bytes!("../web/index.html"),
        "/viewer.mjs" => include_bytes!("../web/viewer.mjs"),
        "/pdf.mjs" => include_bytes!("../web/pdf.mjs"),
        "/office.mjs" => include_bytes!("../web/office.mjs"),
        "/pages.mjs" => include_bytes!("../web/pages.mjs"),
        "/jszip.mjs" => include_bytes!("../web/jszip.mjs"),
        "/viewer.css" => include_bytes!("../web/viewer.css"),
        _ => bundle().ok()?.get(path.strip_prefix('/')?)?,
    };
    let mime = match path.rsplit('.').next()? {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ttf" => "font/ttf",
        "pfb" => "application/octet-stream",
        _ => "application/octet-stream",
    };
    Some((mime, Cow::Borrowed(bytes)))
}

pub(in crate::content::files::html) fn native(
    bytes: Arc<[u8]>,
    mime: &str,
    background: &str,
    status: impl Fn(&str) + 'static,
    window: &Window,
) -> Result<surface::Native, ()> {
    let format = format(mime).ok_or(())?;
    // Wry maps this scheme to http://sailry-preview.localhost on Windows.
    let url = format!("sailry-preview://localhost/index.html#{format}");
    let mime = mime.to_owned();
    let snapshot = crate::ui::webview_snapshot::Snapshot::new();
    let signals = snapshot.signals();
    let builder = wry::WebViewBuilder::new()
        .with_initialization_script(format!(
            "window.previewBackground={};",
            serde_json::to_string(background).unwrap()
        ))
        .with_custom_protocol("sailry-preview".into(), move |_, request| {
            let path = request.uri().path();
            let content = if path == "/document" {
                Some((mime.as_str(), Cow::Owned(bytes.to_vec())))
            } else {
                resource(path)
            };
            let (status, mime, body) = match content {
                Some((mime, bytes)) => (200, mime, bytes),
                None => (404, "text/plain", Cow::Borrowed(&b"Not found"[..])),
            };
            wry::http::Response::builder()
                .status(status)
                .header("Content-Type", mime)
                .header("Content-Security-Policy", CSP)
                .header("X-Content-Type-Options", "nosniff")
                .body(body)
                .unwrap()
        })
        .with_ipc_handler(move |request| {
            if !signals.receive(request.body()) {
                status(request.body());
            }
        })
        .with_navigation_handler(|url| {
            url.starts_with("sailry-preview://localhost/index.html#")
                || url.starts_with("http://sailry-preview.localhost/index.html#")
        })
        .with_url(&url);
    surface::Native::build(builder, window, snapshot)
}
