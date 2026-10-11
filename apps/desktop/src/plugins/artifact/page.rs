//! Trusted immutable package assets plus validated markup in an offline document.
use super::*;
use gpui_kit::component::ActiveTheme;
use serde_json::{Value, json};
#[cfg(test)]
mod tests;

pub(super) fn theme(cx: &App) -> Value {
    let tokens = cx.theme();
    let color = super::super::theme::rgba;
    json!({"background":color(tokens.background),"foreground":color(tokens.foreground),
        "primary":color(tokens.primary),"primary_foreground":color(tokens.primary_foreground),
        "secondary":color(tokens.secondary),"muted":color(tokens.muted),
        "muted_foreground":color(tokens.muted_foreground),"accent":color(tokens.accent),
        "border":color(tokens.border),"success":color(tokens.success),
        "warning":color(tokens.warning),"danger":color(tokens.danger),
        "secondary_foreground":color(tokens.secondary_foreground),
        "font_family":font_family(&tokens.font_family,"system-ui, sans-serif"),"font_size":format!("{}px",f32::from(tokens.font_size)),
        "mono_font_family":font_family(&tokens.mono_font_family,"ui-monospace, monospace"),"radius":format!("{}px",f32::from(tokens.radius)),
        "is_dark":tokens.is_dark()})
}

fn font_family(family: &str, fallback: &str) -> String {
    // GPUI's virtual system family resolves natively, but is not a CSS font name.
    if family == ".SystemUIFont" || family.trim().is_empty() {
        return fallback.into();
    }
    let mut quoted = String::from("\"");
    for character in family.chars() {
        match character {
            '\\' | '"' => {
                quoted.push('\\');
                quoted.push(character);
            }
            '\0' => quoted.push('\u{fffd}'),
            character if character.is_control() => {
                quoted.push_str(&format!("\\{:x} ", character as u32));
            }
            character => quoted.push(character),
        }
    }
    quoted.push_str("\", ");
    quoted.push_str(fallback);
    quoted
}

pub(super) fn labels() -> Value {
    json!({"contents":crate::tr("artifact_contents").to_string(),
        "zoom_in":crate::tr("artifact_zoom_in").to_string(),"zoom_out":crate::tr("artifact_zoom_out").to_string(),
        "close":crate::tr("artifact_close").to_string(),"submit":crate::tr("send").to_string()})
}

pub(super) fn document(
    content: &Content,
    theme: Value,
    labels: Value,
    writable: bool,
    drafts: Vec<crate::preferences::artifacts::Draft>,
) -> Result<String, HostError> {
    use sailry_protocol::artifact::Library;
    let view = content
        .view
        .as_ref()
        .filter(|view| view.valid())
        .ok_or_else(|| HostError::new("artifact view is unavailable"))?;
    if !content.artifact.valid() || content.html.len() > sailry_protocol::artifact::MAX_HTML_BYTES {
        return Err(HostError::new("invalid artifact content"));
    }
    let mut script = String::from(
        "window.sailryDocumentPost=event=>window.ipc.postMessage(JSON.stringify(event));\n",
    );
    if view.libraries.contains(&Library::Mathjax) {
        let asset = |name| {
            sailry_content::markdown::visuals::asset(name)
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .unwrap_or_default()
        };
        script.push_str(asset("config.js"));
        script.push_str("\nMathJax.loader.paths={mathjax:'sailry-document://localhost'};\n");
        script.push_str(asset("mathjax.js"));
        script.push_str(asset("fonts.js"));
    }
    if view.libraries.contains(&Library::Mermaid) {
        script.push_str(
            std::str::from_utf8(sailry_content::markdown::visuals::asset("mermaid.js").unwrap())
                .unwrap(),
        );
    }
    if view.libraries.contains(&Library::Echarts) {
        script.push_str(include_str!("../../../assets/export/echarts.js"));
    }
    for source in &view.scripts {
        script.push('\n');
        script.push_str(source);
    }
    let input = json!({"html":content.html,"theme":theme,"labels":labels,
        "identity":{"artifact":content.artifact.id,"revision":content.artifact.revision,"title":content.artifact.title,"session":content.artifact.session},
        "writable":writable,"drafts":drafts});
    let input = serde_json::to_string(&input)
        .unwrap()
        .replace('<', "\\u003c");
    script.push_str(&format!(
        "\nwindow.sailryDocumentReady=window.sailryDocument.mount({input});\n"
    ));
    let policy = "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; connect-src 'none'; form-action 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'\">";
    let styles: String = view
        .styles
        .iter()
        .map(|style| format!("<style>{}</style>", style.replace("</style", "<\\/style")))
        .collect();
    inject(
        &view.template,
        &format!("{policy}{styles}"),
        &format!(
            "<script>{}</script>",
            script.replace("</script", "<\\/script")
        ),
    )
}

fn inject(template: &str, head: &str, body: &str) -> Result<String, HostError> {
    use html5gum::{DefaultEmitter, Token, Tokenizer};
    let mut emitter = DefaultEmitter::<usize>::new_with_span();
    emitter.naively_switch_states(true);
    let mut start = None;
    let mut end = None;
    for token in Tokenizer::new_with_emitter(template, emitter).flatten() {
        match token {
            Token::StartTag(tag) if tag.name.as_slice() == b"head" && start.is_none() => {
                start = Some(tag.span.end);
            }
            Token::EndTag(tag) if tag.name.as_slice() == b"body" => {
                end = Some(tag.span.start);
                break;
            }
            _ => (),
        }
    }
    let (Some(start), Some(end)) = (start, end) else {
        return Err(HostError::new("artifact template needs head and body tags"));
    };
    if start > end {
        return Err(HostError::new("invalid artifact template"));
    }
    let mut page = template.to_owned();
    page.insert_str(end, body);
    page.insert_str(start, head);
    Ok(page)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn capture(
    content: Content,
    window: &mut Window,
    cx: &mut App,
) -> Task<Result<String, HostError>> {
    use raw_window_handle::HasWindowHandle;
    let html = match document(&content, theme(cx), labels(), false, Vec::new()) {
        Ok(html) => html,
        Err(error) => return Task::ready(Err(error)),
    };
    let script = serialize_script();
    let html = match inject(&html, "", &format!("<script>{script}</script>")) {
        Ok(html) => html,
        Err(error) => return Task::ready(Err(error)),
    };
    let native_handle = match HasWindowHandle::window_handle(window) {
        Ok(handle) => handle,
        Err(_) => return Task::ready(Err(HostError::new("export window is unavailable"))),
    };
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let native = wry::WebViewBuilder::new()
        .with_html(html)
        .with_incognito(true)
        .with_visible(false)
        .with_background_throttling(wry::BackgroundThrottlingPolicy::Disabled)
        .with_bounds(wry::Rect {
            position: wry::dpi::LogicalPosition::new(0., 0.).into(),
            size: wry::dpi::LogicalSize::new(960., 768.).into(),
        })
        .with_navigation_handler(|url| url == "about:blank")
        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
        .with_download_started_handler(|_, _| false)
        .with_ipc_handler(move |request| {
            if request.body().len() <= 16 * 1024 * 1024 {
                let Ok(value) = serde_json::from_str::<Value>(request.body()) else {
                    return;
                };
                if value
                    .get("kind")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| matches!(kind, "export" | "export_error"))
                {
                    let _ = sender.try_send(value);
                }
            }
        })
        .build_as_child(&native_handle);
    let Ok(native) = native else {
        return Task::ready(Err(HostError::new("artifact export failed")));
    };
    window.spawn(cx, async move |cx| {
        let _native = native;
        let timer = cx
            .background_executor()
            .timer(std::time::Duration::from_secs(60));
        let result = tokio::select! {result=receiver.recv()=>result, _=timer=>None};
        result
            .and_then(|value| value.get("html").and_then(Value::as_str).map(str::to_owned))
            .ok_or_else(|| HostError::new("artifact export failed"))
    })
}

pub(super) fn serialize_script() -> String {
    serialize_capture(None)
}

pub(super) fn serialize_capture(capture: Option<u64>) -> String {
    use base64::Engine as _;
    let logo = base64::engine::general_purpose::STANDARD.encode(include_bytes!(
        "../../../../../assets/branding/sailry-mark.svg"
    ));
    let qr = base64::engine::general_purpose::STANDARD.encode(include_bytes!(
        "../../../../../assets/branding/sailry-site-qr.svg"
    ));
    let footer = format!(
        "<footer style=\"margin-top:40px;padding:24px;display:flex;gap:16px;align-items:center\"><img width=48 height=48 alt=Sailry src=\"data:image/svg+xml;base64,{logo}\"><div style=\"flex:1\"><strong>Sailry</strong><br><a href=\"https://sailry.dev\">sailry.dev</a></div><img width=96 height=96 alt=\"https://sailry.dev\" src=\"data:image/svg+xml;base64,{qr}\"></footer>"
    );
    let footer = serde_json::to_string(&footer)
        .unwrap()
        .replace('<', "\\u003c");
    let capture = serde_json::to_string(&capture).unwrap();
    format!(
        r#"window.sailryDocumentReady.then(async()=>{{await document.fonts.ready;const body=await window.sailryDocument.readonlyExport();const clone=document.documentElement.cloneNode(true);clone.querySelectorAll('script,dialog,nav,meta[http-equiv]').forEach(node=>node.remove());clone.querySelector('body').innerHTML=body+{footer};window.ipc.postMessage(JSON.stringify({{kind:'export',capture:{capture},html:'<!doctype html>'+clone.outerHTML}}));}}).catch(()=>window.ipc.postMessage(JSON.stringify({{kind:'export_error',capture:{capture}}})));"#
    )
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(super) fn capture(_: Content, _: &mut Window, _: &mut App) -> Task<Result<String, HostError>> {
    Task::ready(Err(HostError::new("native artifact export is unavailable")))
}
