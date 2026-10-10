//! Offline HTML rendering uses the existing native WebView boundary.
use base64::Engine as _;
use gpui_kit::{App, Task, Window};
use std::path::PathBuf;

#[cfg(all(feature = "workload-tests", target_os = "macos"))]
pub(crate) mod check;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native;

pub(crate) fn assets(directory: &std::path::Path) -> Result<(), &'static str> {
    for (name, source) in [
        (
            "config.js",
            sailry_content::markdown::visuals::asset("config.js").unwrap(),
        ),
        (
            "mathjax.js",
            sailry_content::markdown::visuals::asset("mathjax.js").unwrap(),
        ),
        (
            "fonts.js",
            sailry_content::markdown::visuals::asset("fonts.js").unwrap(),
        ),
        (
            "mermaid.js",
            sailry_content::markdown::visuals::asset("mermaid.js").unwrap(),
        ),
        (
            "echarts.js",
            include_bytes!("../../../../assets/export/echarts.js").as_slice(),
        ),
        (
            "prepare.js",
            include_bytes!("../../../../assets/export/prepare.js").as_slice(),
        ),
        (
            "html2canvas.js",
            include_bytes!("../../../../assets/export/html2canvas.js").as_slice(),
        ),
        (
            "capture.js",
            include_bytes!("../../../../assets/export/capture.js").as_slice(),
        ),
        (
            "echarts-LICENSE",
            include_bytes!("../../../../assets/export/echarts-LICENSE").as_slice(),
        ),
        (
            "echarts-NOTICE",
            include_bytes!("../../../../assets/export/echarts-NOTICE").as_slice(),
        ),
        (
            "html2canvas-LICENSE",
            include_bytes!("../../../../assets/export/html2canvas-LICENSE").as_slice(),
        ),
        (
            "mathjax-LICENSE",
            include_bytes!("../../../../../../crates/content/assets/visuals/licenses/mathjax.txt")
                .as_slice(),
        ),
        (
            "mathjax-font-LICENSE",
            include_bytes!(
                "../../../../../../crates/content/assets/visuals/licenses/mathjax-font.txt"
            )
            .as_slice(),
        ),
        (
            "mermaid-LICENSE",
            include_bytes!("../../../../../../crates/content/assets/visuals/licenses/mermaid.txt")
                .as_slice(),
        ),
    ] {
        std::fs::write(directory.join(name), source).map_err(|_| "files_download_destination")?;
    }
    Ok(())
}

pub(super) fn decorate(html: &str, assets: &str) -> String {
    let scripts: String = [
        "config.js",
        "mathjax.js",
        "fonts.js",
        "mermaid.js",
        "echarts.js",
        "prepare.js",
    ]
    .into_iter()
    .map(|name| format!("<script src=\"{assets}/{name}\"></script>"))
    .collect();
    let logo = base64::engine::general_purpose::STANDARD.encode(include_bytes!(
        "../../../../../../assets/branding/sailry-mark.svg"
    ));
    let qr = base64::engine::general_purpose::STANDARD.encode(include_bytes!(
        "../../../../../../assets/branding/sailry-site-qr.svg"
    ));
    let html = html.replace("</body>", &format!(r#"<footer style="margin-top:40px;padding:28px;background:#f3f4f6;display:flex;align-items:center;gap:18px"><img alt="Sailry" width="60" height="60" src="data:image/svg+xml;base64,{logo}"><div style="flex:1"><strong style="font-size:24px">Sailry</strong><br><a href="https://sailry.dev" style="color:#596273;text-decoration:none">sailry.dev</a></div><img alt="https://sailry.dev" width="112" height="112" style="background:#fff" src="data:image/svg+xml;base64,{qr}"></footer></body>"#));
    html.replace("</head>", &format!("<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; script-src 'self'; style-src 'unsafe-inline'; img-src 'self' data:; font-src data:; connect-src 'none'; frame-src 'self'; base-uri 'none'; object-src 'none'\"><style>html{{background:#fff;color:#20242b}}body{{max-width:900px;margin:0 auto;padding:32px;font:18px/1.7 system-ui}}pre{{white-space:pre-wrap;overflow-wrap:anywhere}}table{{width:100%;font-size:15px}}.export-visual{{max-width:100%;height:auto}}mjx-container{{max-width:100%;overflow:auto}}</style>{scripts}</head>"))
}

pub(crate) fn render(
    html: String,
    assets: String,
    directory: PathBuf,
    window: &mut Window,
    cx: &mut App,
) -> Task<Result<Vec<u8>, &'static str>> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        native::render(html, assets, directory, window, cx)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (html, assets, directory, window, cx);
        Task::ready(Err("conversation_export_failed"))
    }
}
