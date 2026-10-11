//! Opt-in native static HTML and PNG acceptance with bundled assets.
use super::*;
use gpui_kit::{component::Root, *};

pub(crate) fn run(output: &std::path::Path) {
    let output = output.to_owned();
    gpui_kit::application().with_assets(crate::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        let output = output.clone();
        cx.spawn(async move |cx| {
            let _ = crate::workload::check(async {
            let window = cx.open_window(WindowOptions::default(), |window, cx| {
                let content = cx.new(|_| Empty);
                cx.new(|cx| Root::new(content, window, cx))
            }).unwrap();
            let directory = output.join("sailry-assets-fixture");
            std::fs::create_dir_all(&directory).unwrap();
            let body = format!("<!doctype html><html><head><meta charset=\"utf-8\"></head><body><h1>Unicode 中文 🙂</h1><p>\\(x^2 + y^2 = z^2\\)</p><pre><code class=\"language-chart\">{{\"type\":\"bar\",\"title\":\"Sales\",\"data\":[{{\"label\":\"A\",\"value\":12}},{{\"label\":\"B\",\"value\":24}}]}}</code></pre><pre><code class=\"language-mermaid\">graph TD\nStart --&gt; Complete</code></pre><a href=\"sailry-assets-fixture/document.txt\">document.txt</a><table>{}</table></body></html>", "<tr><td>Long export</td><td>完整正文</td></tr>".repeat(120));
            assets(&directory).unwrap();
            std::fs::write(directory.join("document.txt"), "Captured attachment bytes").unwrap();
            let html = decorate(&body, "sailry-assets-fixture");
            std::fs::create_dir_all(&output).unwrap();
            let task = window.update(cx, |_, window, cx| render_html(html.clone(), "sailry-assets-fixture".into(), directory.clone(), window, cx)).unwrap();
            let bytes = task.await.expect("Native static HTML export must succeed");
            let document = std::str::from_utf8(&bytes).unwrap();
            assert!(document.contains("<mjx-container"));
            assert!(document.contains("export-diagram-0"));
            assert!(document.contains("export-visual"));
            assert!(document.matches("<svg").count() >= 3);
            assert!(!document.contains("<script"));
            assert!(!document.contains("language-mermaid"));
            assert!(!document.contains("language-chart"));
            assert!(!document.contains("sailry-assets-fixture/"));
            assert!(document.contains("data:text/plain;base64,Q2FwdHVyZWQgYXR0YWNobWVudCBieXRlcw=="));
            assert!(document.contains("script-src 'none'"));
            std::fs::write(output.join("reply.html"), bytes).unwrap();
            let task = window.update(cx, |_, window, cx| render(html, "sailry-assets-fixture".into(), directory.clone(), window, cx)).unwrap();
            let bytes = task.await.expect("Native HTML export must succeed");
            let bitmap = ::image::load_from_memory(&bytes).unwrap();
            assert_eq!(bitmap.width(), 960);
            assert!(bitmap.height() > 4096);
            std::fs::create_dir_all(&output).unwrap();
            std::fs::write(output.join("reply.png"), bytes).unwrap();
            std::fs::remove_dir_all(&directory).unwrap();
            offline(&window, &output.join("reply.html"), cx).await;
            println!("Native export passed: offline static HTML with formula, diagram, chart and embedded attachment; PNG 960 x {} pixels with tiled table", bitmap.height());
            }).await;
            cx.update(|cx| cx.quit());
        }).detach();
    });
}

async fn offline(window: &WindowHandle<Root>, path: &std::path::Path, cx: &mut AsyncApp) {
    use raw_window_handle::HasWindowHandle;
    let url = reqwest::Url::from_file_path(path).unwrap().to_string();
    let view = window
        .update(cx, |_, window, _| {
            let handle = HasWindowHandle::window_handle(window).unwrap();
            wry::WebViewBuilder::new()
                .with_url(url)
                .with_visible(false)
                .with_incognito(true)
                .build_as_child(&handle)
                .unwrap()
        })
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let sender = std::sync::Mutex::new(Some(sender));
        view.evaluate_script_with_callback(r#"document.readyState === 'complete' && document.querySelectorAll('svg').length >= 3 && [...document.querySelectorAll('mjx-container svg, .export-visual, #export-diagram-0')].every(svg => svg.getBoundingClientRect().width > 0 && svg.getBoundingClientRect().height > 0) && [...document.querySelectorAll('svg use')].every(use => document.getElementById((use.getAttribute('href') || use.getAttribute('xlink:href') || '').slice(1))) && document.querySelectorAll('script').length === 0"#, move |value| {
            if let Some(sender) = sender.lock().unwrap().take() { let _ = sender.send(value); }
        }).unwrap();
        let timer = cx
            .background_executor()
            .timer(std::time::Duration::from_millis(200));
        let ready = tokio::select! { result = receiver => result.is_ok_and(|value| value == "true"), _ = timer => false };
        if ready {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "offline static HTML rendering deadline"
        );
        cx.background_executor()
            .timer(std::time::Duration::from_millis(100))
            .await;
    }
}
struct Empty;
impl Render for Empty {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
