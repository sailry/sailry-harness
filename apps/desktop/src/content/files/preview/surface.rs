use super::*;
use raw_window_handle::HasWindowHandle as _;
#[cfg(test)]
#[path = "overlay_tests.rs"]
mod overlays;

// Native child views bypass Kit's hitbox and paint order. Leave its divider's
// right-hand grab band clear: pinned Kit uses 4px padding plus a 1px hairline.
const RESIZE_CLEARANCE: Pixels = px(5.);

pub(crate) struct Native {
    pub(crate) page: gpui_webview::WebViewHandle,
    view: Entity<gpui_webview::WebView>,
}
impl Native {
    pub(crate) fn new(html: &str, window: &mut Window, cx: &mut App) -> Result<Self, ()> {
        let builder = wry::WebViewBuilder::new()
            .with_html(document(html, cx))
            .with_navigation_handler(|url| url == "about:blank" || url == "about:srcdoc");
        Self::build(builder, window, cx)
    }

    pub(crate) fn build(
        builder: wry::WebViewBuilder<'_>,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<Self, ()> {
        #[cfg(all(feature = "workload-tests", target_os = "macos"))]
        let builder = super::check::prepare(builder);
        let handle = window.window_handle().map_err(|_| ())?;
        let page = builder
            .with_incognito(true)
            .with_focused(false)
            .with_visible(false)
            .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
            .with_download_started_handler(|_, _| false)
            .build_as_child(&handle)
            .map_err(|_| ())?;
        let view = cx.new(|cx| {
            let mut page = gpui_webview::WebView::new(page, window, cx);
            page.hide();
            page
        });
        let page = view.read(cx).handle();
        Ok(Self { page, view })
    }

    pub(crate) fn set_visible(&self, visible: bool, cx: &mut App) -> wry::Result<()> {
        self.view.update(cx, |page, _| page.set_visible(visible))
    }
}

struct Mount;

pub(crate) fn render(native: Rc<Native>, window: &mut Window, cx: &mut App) -> AnyElement {
    let page = native.view.downgrade();
    let key: SharedString = format!("file-preview-mount-{}", native.view.entity_id()).into();
    window.use_keyed_state(key, cx, move |_, cx| {
        cx.on_release(move |_: &mut Mount, cx| {
            if let Some(page) = page.upgrade() {
                page.update(cx, |page, _| page.hide());
            }
        })
        .detach();
        Mount
    });
    native.view.update(cx, |page, _| page.show());
    div()
        .size_full()
        .min_w_0()
        .min_h_0()
        .pl(RESIZE_CLEARANCE)
        .child(native.view.clone())
        .into_any_element()
}

pub(crate) fn render_hidden(native: Rc<Native>, cx: &mut App) -> AnyElement {
    let _ = native.set_visible(false, cx);
    div()
        .size_full()
        .min_w_0()
        .min_h_0()
        .pl(RESIZE_CLEARANCE)
        .child(native.view.clone())
        .into_any_element()
}

pub(crate) fn document(html: &str, cx: &App) -> String {
    use gpui_kit::component::ActiveTheme;
    let color = |value: Hsla| {
        let value = Rgba::from(value);
        format!(
            "rgba({},{},{},{})",
            value.r * 255.,
            value.g * 255.,
            value.b * 255.,
            value.a
        )
    };
    wrapper(
        html,
        &color(cx.theme().background),
        &color(cx.theme().foreground),
    )
}

fn wrapper(html: &str, background: &str, foreground: &str) -> String {
    // srcdoc inherits the parent's policy. Its own policy only tightens it;
    // frame-src on the parent also blocks navigation of the authored frame.
    // WebRTC blocking is policy intent; native engine support is platform-dependent.
    const POLICY: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; connect-src 'none'; form-action 'none'; worker-src 'none'; object-src 'none'; base-uri 'none'; webrtc 'block'";
    let content = format!(
        "<meta http-equiv=\"Content-Security-Policy\" content=\"{POLICY}; frame-src 'none'\">{html}"
    );
    let escaped = content
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        "<!doctype html><html><head><meta charset=utf-8><meta http-equiv=\"Content-Security-Policy\" content=\"{POLICY}; frame-src about:\"><style>html,body{{margin:0;overflow:hidden;background:{background};color:{foreground}}}iframe{{position:absolute;border:0;width:100vw;height:100vh}}</style></head><body><iframe id=preview sandbox=\"allow-scripts\" allow=\"camera 'none'; microphone 'none'; geolocation 'none'; clipboard-read 'none'\" referrerpolicy=\"no-referrer\" srcdoc=\"{escaped}\"></iframe></body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[test]
    fn confines_interactive_content() {
        let html = wrapper(
            "</iframe><script>parent.postMessage('test','*')</script>",
            "inherit",
            "inherit",
        );
        assert_eq!(html.matches("<iframe ").count(), 1);
        assert_eq!(html.matches("</iframe>").count(), 1);
        assert!(html.contains("sandbox=\"allow-scripts\""));
        assert!(!html.contains("allow-same-origin"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("connect-src 'none'"));
        assert!(html.contains("frame-src about:"));
        assert!(html.contains("frame-src 'none'"));
        assert!(html.contains("worker-src 'none'"));
        assert_eq!(html.matches("webrtc 'block'").count(), 2);
        assert!(html.contains("clipboard-read 'none'"));
        assert!(!html.contains("window.ipc"));
        assert!(!html.contains("allow-popups"));
        assert!(!html.contains("allow-downloads"));
    }
}
