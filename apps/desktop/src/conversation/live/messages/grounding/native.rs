use super::*;
use gpui_kit::component::link::Link;
use gpui_wry::WebView;
use raw_window_handle::HasWindowHandle;

struct ResultView {
    entry: Arc<Entry>,
    webview: Option<Entity<WebView>>,
    _links: Task<()>,
}

pub(super) fn open(entry: Arc<Entry>, cx: &mut App) -> bool {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(720.), px(620.)), cx)),
        window_min_size: Some(size(px(360.), px(360.))),
        ..Default::default()
    };
    if let Err(error) = cx.open_window(options, |window, cx| {
        window.set_window_title(&tr("chat_search_result"));
        let view = cx.new(|cx| ResultView::new(entry, window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    }) {
        eprintln!("sailry-desktop: failed to open search result: {error}");
        return false;
    }
    true
}

impl ResultView {
    fn new(entry: Arc<Entry>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (links, mut incoming) = tokio::sync::mpsc::unbounded_channel::<String>();
        let popups = links.clone();
        let html = format!(
            "<!doctype html><meta charset=utf-8><meta http-equiv=Content-Security-Policy content=\"default-src 'none'; style-src 'unsafe-inline'; img-src https: data:; form-action 'none'; base-uri 'none'\">{}",
            entry.search_suggestions.as_deref().unwrap_or_default(),
        );
        let webview = window
            .window_handle()
            .ok()
            .and_then(|handle| {
                wry::WebViewBuilder::new()
                    .with_html(html)
                    .with_transparent(true)
                    .with_javascript_disabled()
                    .with_incognito(true)
                    .with_download_started_handler(|_, _| false)
                    .with_navigation_handler(move |url| {
                        if url == "about:blank" {
                            return true;
                        }
                        if destination(&url) {
                            let _ = links.send(url);
                        }
                        false
                    })
                    .with_new_window_req_handler(move |url, _| {
                        if destination(&url) {
                            let _ = popups.send(url);
                        }
                        wry::NewWindowResponse::Deny
                    })
                    .build_as_child(&handle)
                    .map_err(|error| {
                        eprintln!("sailry-desktop: failed to render search suggestions: {error}")
                    })
                    .ok()
            })
            .map(|raw| cx.new(|cx| WebView::new(raw, window, cx)));
        let task = cx.spawn(async move |_, cx| {
            while let Some(url) = incoming.recv().await {
                cx.update(|cx| cx.open_url(&url));
            }
        });
        let owner = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            let _ = owner.update(cx, |view, _| {
                view.webview.take();
            });
            // Release native child handles held by the previous GPUI frame before closing.
            window.refresh();
            window.draw(cx).clear(cx);
            true
        });
        Self {
            entry,
            webview,
            _links: task,
        }
    }
}

impl Render for ResultView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(webview) = &self.webview else {
            return div()
                .p_4()
                .child(tr("chat_search_unavailable"))
                .into_any_element();
        };
        // Kit's native WebView overlays GPUI; give it its own window and fixed region.
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .id("search-answer")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .child(
                        crate::content::markdown::View::markdown(
                            "grounded-answer",
                            answer(&self.entry),
                        )
                        .on_link_click(|url, _, cx| {
                            if destination(url) {
                                cx.open_url(url);
                            }
                        }),
                    )
                    .children(
                        self.entry
                            .citations
                            .iter()
                            .enumerate()
                            .map(|(index, source)| {
                                div().mt_2().child(
                                    Link::new(("search-source", index))
                                        .href(source.uri.clone())
                                        .child(
                                            source
                                                .title
                                                .clone()
                                                .unwrap_or_else(|| source.uri.clone()),
                                        ),
                                )
                            }),
                    ),
            )
            .child(Separator::horizontal())
            .child(div().h(px(180.)).flex_shrink_0().child(webview.clone()))
            .into_any_element()
    }
}
