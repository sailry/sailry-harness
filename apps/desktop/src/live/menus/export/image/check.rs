//! Opt-in native HTML-to-PNG acceptance with bundled assets and an isolated profile.
use super::*;
use gpui_kit::{component::Root, *};

pub(crate) fn run(output: &std::path::Path) {
    let output = output.to_owned();
    gpui_kit::application().with_assets(crate::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        let output = output.clone();
        cx.spawn(async move |cx| {
            let window = cx.open_window(WindowOptions::default(), |window, cx| {
                let content = cx.new(|_| Empty);
                cx.new(|cx| Root::new(content, window, cx))
            }).unwrap();
            let directory = output.join("assets");
            std::fs::create_dir_all(&directory).unwrap();
            let body = format!("<!doctype html><html><head><meta charset=\"utf-8\"></head><body><h1>Unicode 中文 🙂</h1><p>\\(x^2 + y^2 = z^2\\)</p><pre><code class=\"language-chart\">{{\"type\":\"bar\",\"title\":\"Sales\",\"data\":[{{\"label\":\"A\",\"value\":12}},{{\"label\":\"B\",\"value\":24}}]}}</code></pre><table>{}</table></body></html>", "<tr><td>Long export</td><td>完整正文</td></tr>".repeat(120));
            assets(&directory).unwrap();
            let html = decorate(&body, "assets");
            std::fs::create_dir_all(&output).unwrap();
            std::fs::write(output.join("reply.html"), &html).unwrap();
            let task = window.update(cx, |_, window, cx| render(html, "assets".into(), directory, window, cx)).unwrap();
            let bytes = task.await.expect("Native HTML export must succeed");
            let bitmap = ::image::load_from_memory(&bytes).unwrap();
            assert_eq!(bitmap.width(), 960);
            assert!(bitmap.height() > 4096);
            std::fs::create_dir_all(&output).unwrap();
            std::fs::write(output.join("reply.png"), bytes).unwrap();
            println!("Native HTML export passed: 960 x {} pixels, Unicode, formula, chart and tiled table", bitmap.height());
            cx.update(|cx| cx.quit());
        }).detach();
    });
}
struct Empty;
impl Render for Empty {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
