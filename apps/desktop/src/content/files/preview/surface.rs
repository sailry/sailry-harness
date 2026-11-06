use super::*;
use crate::theme::css::Palette;
use raw_window_handle::HasWindowHandle as _;

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
        "",
        &format!(
            "<style>html,body{{margin:0;overflow:hidden;background:{};color:{}}}{FILL}</style>",
            color(cx.theme().background),
            color(cx.theme().foreground)
        ),
    )
}

/// Authored artifact previews may follow the application theme through
/// `--sailry-*` variables; their own CSS still wins.
pub(crate) fn artifact(html: &str, palette: &Palette) -> String {
    let rule = palette.rule();
    wrapper(
        html,
        &format!("{}<script>{THEME}</script>", themed(palette)),
        &format!(
            "<style id=sailry-theme>{rule}</style><style>html,body{{margin:0;overflow:hidden;background:var(--sailry-background);color:var(--sailry-foreground)}}{FILL}</style>"
        ),
    )
}

/// Moves a mounted artifact preview to `palette` without reloading authored state.
pub(crate) fn retheme(palette: &Palette) -> String {
    let rule = serde_json::to_string(&palette.rule()).expect("CSS text serializes as JSON");
    format!(
        "(()=>{{const rule={rule};const style=document.getElementById('sailry-theme');if(style)style.textContent=rule;document.getElementById('preview')?.contentWindow?.postMessage({{sailryTheme:rule}},'*');}})();"
    )
}

/// A thumbnail page with no native message channel. The authored frame reports
/// its content box to this page, which the renderer polls and then places.
pub(crate) fn snapshot(html: &str, palette: &Palette, width: f32, height: f32, job: u64) -> String {
    let rule = palette.rule();
    wrapper(
        html,
        &format!("{}<script>{MEASURE}</script>", themed(palette)),
        &format!(
            "<style id=sailry-theme>{rule}</style><style>html,body{{margin:0;overflow:hidden;width:100%;height:100%}}body{{background-color:var(--sailry-background);background-image:radial-gradient(circle,var(--sailry-border) 1px,transparent 1.5px);background-size:16px 16px;background-position:8px 8px}}iframe{{position:absolute;left:0;top:0;border:0;width:{width}px;height:{height}px}}</style><script>window.sailryJob={job};{PLACE}</script>"
        ),
    )
}

const FILL: &str = "iframe{position:absolute;border:0;width:100vw;height:100vh}";
const THEME: &str = "addEventListener('message',event=>{const rule=event.data&&event.data.sailryTheme;if(event.source!==parent||typeof rule!=='string')return;const style=document.getElementById('sailry-theme');if(style)style.textContent=rule;});";
// Waits for fonts and a short settle so entry animations and charts can draw.
const MEASURE: &str = r#"(()=>{const wait=ms=>new Promise(done=>setTimeout(done,ms));const frame=()=>new Promise(done=>requestAnimationFrame(()=>requestAnimationFrame(done)));const painted=element=>{if(!element)return false;const style=getComputedStyle(element);if(style.backgroundImage!=='none')return true;const channels=style.backgroundColor.match(/[\d.]+/g);return !!channels&&(channels.length<4||Number(channels[3])>0)};const measure=()=>{const body=document.body,root=document.documentElement;let left=Infinity,top=Infinity,right=-Infinity,bottom=-Infinity;const add=box=>{if(box.width<1||box.height<1)return;left=Math.min(left,box.left);top=Math.min(top,box.top);right=Math.max(right,box.right);bottom=Math.max(bottom,box.bottom)};if(body){for(const element of body.children){if(!/^(SCRIPT|STYLE|TEMPLATE|LINK|META|NOSCRIPT)$/.test(element.tagName))add(element.getBoundingClientRect())}if(right<left){const range=document.createRange();range.selectNodeContents(body);add(range.getBoundingClientRect())}}const found=right>left&&bottom>top;return{left:found?left:0,top:found?top:0,width:found?right-left:0,height:found?bottom-top:0,scroll_height:root?root.scrollHeight:0,opaque:painted(root)||painted(body)}};addEventListener('load',()=>{Promise.race([document.fonts?document.fonts.ready:null,wait(1500)]).then(()=>wait(300)).then(frame).then(()=>parent.postMessage({sailrySnapshot:measure()},'*'))},{once:true})})();"#;
// The first report wins; authored code shares that frame, so values stay bounded.
const PLACE: &str = "window.sailrySnapshot=null;window.sailryPlaced=false;addEventListener('message',event=>{const frame=document.getElementById('preview');const value=event.data&&event.data.sailrySnapshot;if(!frame||event.source!==frame.contentWindow||window.sailrySnapshot||!value||typeof value!=='object')return;const number=key=>{const result=Number(value[key]);return Number.isFinite(result)?Math.max(-100000,Math.min(100000,result)):0};window.sailrySnapshot={left:number('left'),top:number('top'),width:number('width'),height:number('height'),scroll_height:number('scroll_height'),opaque:value.opaque===true}});window.sailryPlace=(x,y)=>{document.getElementById('preview').style.transform='translate('+x+'px,'+y+'px)';requestAnimationFrame(()=>requestAnimationFrame(()=>{window.sailryPlaced=true}))};";

fn themed(palette: &Palette) -> String {
    format!(
        "<meta name=color-scheme content={}><style id=sailry-theme>{}</style>",
        palette.scheme(),
        palette.rule()
    )
}

/// `frame` precedes authored code inside the sandboxed srcdoc; `page` is the
/// Sailry-owned top-level document around it.
fn wrapper(html: &str, frame: &str, page: &str) -> String {
    // srcdoc inherits the parent's policy. Its own policy only tightens it;
    // frame-src on the parent also blocks navigation of the authored frame.
    // WebRTC blocking is policy intent; native engine support is platform-dependent.
    const POLICY: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; connect-src 'none'; form-action 'none'; worker-src 'none'; object-src 'none'; base-uri 'none'; webrtc 'block'";
    let content = format!(
        "<meta http-equiv=\"Content-Security-Policy\" content=\"{POLICY}; frame-src 'none'\">{frame}{html}"
    );
    let escaped = content
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        "<!doctype html><html><head><meta charset=utf-8><meta http-equiv=\"Content-Security-Policy\" content=\"{POLICY}; frame-src about:\">{page}</head><body><iframe id=preview sandbox=\"allow-scripts\" allow=\"camera 'none'; microphone 'none'; geolocation 'none'; clipboard-read 'none'\" referrerpolicy=\"no-referrer\" srcdoc=\"{escaped}\"></iframe></body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    const AUTHORED: &str = "</iframe><script>parent.postMessage('test','*')</script>";

    fn palette() -> Palette {
        Palette {
            dark: true,
            variables: vec![
                ("background", "#000000ff".into()),
                ("border", "#ffffff22".into()),
            ],
        }
    }

    fn confined(html: &str) {
        assert_eq!(html.matches("<iframe ").count(), 1);
        assert_eq!(html.matches("</iframe>").count(), 1);
        assert!(html.contains("sandbox=\"allow-scripts\""));
        assert!(!html.contains("allow-same-origin"));
        assert!(html.contains("&lt;script&gt;parent.postMessage"));
        assert!(html.contains("connect-src 'none'"));
        assert!(html.contains("frame-src about:"));
        assert!(html.contains("frame-src 'none'"));
        assert!(html.contains("worker-src 'none'"));
        assert_eq!(html.matches("webrtc 'block'").count(), 2);
        assert!(html.contains("clipboard-read 'none'"));
        assert!(!html.contains("window.ipc"));
        assert!(!html.contains("sailryDocument"));
        assert!(!html.contains("allow-popups"));
        assert!(!html.contains("allow-downloads"));
    }

    /// The escaped srcdoc, so assertions can tell frame content from the page.
    fn frame(html: &str) -> &str {
        let start = html.find("srcdoc=\"").unwrap() + "srcdoc=\"".len();
        &html[start..html.rfind("\"></iframe>").unwrap()]
    }

    #[test]
    fn confines_interactive_content() {
        let plain = wrapper(AUTHORED, "", "<style>html{color:inherit}</style>");
        confined(&plain);
        assert!(!plain.contains("--sailry-"));
        confined(&artifact(AUTHORED, &palette()));
        confined(&snapshot(AUTHORED, &palette(), 640., 408., 7));
    }

    #[test]
    fn themes_both_documents() {
        let html = artifact(AUTHORED, &palette());
        let rule = palette().rule();
        let frame = frame(&html);
        assert!(html[..html.find("<iframe").unwrap()].contains(&rule));
        assert!(frame.contains(&rule));
        assert!(frame.contains("name=color-scheme content=dark"));
        assert!(frame.contains("sailryTheme"));
        assert!(frame.find("sailry-theme").unwrap() < frame.find("&lt;/iframe&gt;").unwrap());
        assert!(!frame.contains("sailrySnapshot"));
    }

    #[test]
    fn snapshot_reports_without_channel() {
        let html = snapshot(AUTHORED, &palette(), 640., 408., 7);
        let page = &html[..html.find("<iframe").unwrap()];
        let frame = frame(&html);
        assert!(page.contains("window.sailryJob=7;"));
        assert!(page.contains("window.sailryPlace="));
        assert!(page.contains("width:640px;height:408px"));
        assert!(page.contains("radial-gradient"));
        assert!(frame.contains("parent.postMessage({sailrySnapshot:measure()},'*')"));
        assert!(!frame.contains("sailryPlace"));
        assert!(!frame.contains("sailryTheme"));
        assert!(frame.find("sailrySnapshot").unwrap() < frame.find("&lt;/iframe&gt;").unwrap());
    }

    #[test]
    fn retheme_posts_rule_to_frame() {
        let script = retheme(&palette());
        let rule = serde_json::to_string(&palette().rule()).unwrap();
        assert!(script.contains(&format!("const rule={rule};")));
        assert!(script.contains("postMessage({sailryTheme:rule},'*')"));
        assert!(!script.contains("ipc"));
    }
}
