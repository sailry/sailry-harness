use super::*;

pub(super) struct Native {
    pub(super) page: wry::WebView,
    pub(super) frame: Cell<Option<(Bounds<Pixels>, Bounds<Pixels>)>>,
    visible: Cell<bool>,
    #[cfg(target_os = "macos")]
    _pointer: Rc<pointer::Pointer>,
}
impl Native {
    pub(super) fn new(html: &str, window: &Window) -> Result<Self, ()> {
        Self::build(
            wry::WebViewBuilder::new()
                .with_html(document(html))
                .with_navigation_handler(|url| url == "about:blank" || url == "about:srcdoc"),
            window,
        )
    }

    pub(super) fn build(builder: wry::WebViewBuilder<'_>, window: &Window) -> Result<Self, ()> {
        #[cfg(all(feature = "workload-tests", target_os = "macos"))]
        let builder = super::check::prepare(builder);
        let handle = HasWindowHandle::window_handle(window).map_err(|_| ())?;
        let page = builder
            .with_visible(false)
            .with_incognito(true)
            .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
            .with_download_started_handler(|_, _| false)
            .build_as_child(&handle)
            .map_err(|_| ())?;
        Ok(Self {
            #[cfg(target_os = "macos")]
            _pointer: pointer::Pointer::attach(&page),
            page,
            frame: Cell::new(None),
            visible: Cell::new(false),
        })
    }
    fn show(&self, visible: bool) {
        if self.visible.get() != visible && self.page.set_visible(visible).is_ok() {
            self.visible.set(visible);
        }
    }
    fn hide(&self) {
        if self.visible.get() {
            #[cfg(target_os = "macos")]
            pointer::release_focus(&self.page);
            #[cfg(not(target_os = "macos"))]
            let _ = self.page.focus_parent();
        }
        self.show(false);
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        self.hide();
    }
}

struct Mount(Rc<Native>);
impl Drop for Mount {
    fn drop(&mut self) {
        self.0.hide();
    }
}

pub(super) fn render(native: Rc<Native>, window: &mut Window, cx: &mut App) -> AnyElement {
    let mounted = native.clone();
    // Kit element state lasts through cached frames and is released on unmount,
    // even when the owning session retains its preview entity in the background.
    let key: SharedString = format!("file-preview-mount-{:p}", Rc::as_ptr(&native)).into();
    window.use_keyed_state(key, cx, move |_, _| Mount(mounted));
    canvas(|bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal), move |bounds, _, window, cx| {
        let blocked = gpui_kit::base::GlobalState::is_in_deferred_context(cx)
            || window.has_active_dialog(cx) || window.has_active_sheet(cx);
        let clip = bounds.intersect(&window.content_mask().bounds);
        if blocked || clip.size.width <= px(0.) || clip.size.height <= px(0.) {
            native.hide(); return;
        }
        let previous = native.frame.get();
        if previous.is_none_or(|(_, previous_clip)| previous_clip != clip) {
            let _ = native.page.set_bounds(wry::Rect {
                position: wry::dpi::LogicalPosition::new(f64::from(f32::from(clip.left())), f64::from(f32::from(clip.top()))).into(),
                size: wry::dpi::LogicalSize::new(f64::from(f32::from(clip.size.width)), f64::from(f32::from(clip.size.height))).into(),
            });
        }
        if previous.is_none_or(|(previous_bounds, previous_clip)| layout(previous_bounds, previous_clip) != layout(bounds, clip)) {
            let script = format!("window.previewFrame={{width:'{}px',height:'{}px',left:'{}px',top:'{}px'}};window.layoutPreview?.();",
                f32::from(bounds.size.width), f32::from(bounds.size.height), f32::from(bounds.left()-clip.left()), f32::from(bounds.top()-clip.top()));
            let _ = native.page.evaluate_script(&script);
        }
        native.frame.set(Some((bounds, clip)));
        native.show(true);
        window.on_mouse_event(move |event: &MouseDownEvent, phase, _, _| {
            if phase == DispatchPhase::Capture && !clip.contains(&event.position) {
                let _ = native.page.focus_parent();
            }
        });
    }).size_full().into_any_element()
}

fn layout(bounds: Bounds<Pixels>, clip: Bounds<Pixels>) -> (gpui_kit::Size<Pixels>, Point<Pixels>) {
    (bounds.size, bounds.origin - clip.origin)
}

fn document(html: &str) -> String {
    let content = format!(
        "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; connect-src 'none'; form-action 'none'; base-uri 'none'\">{html}"
    );
    let escaped = content
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        "<!doctype html><html><head><meta charset=utf-8><style>html,body{{margin:0;overflow:hidden}}iframe{{position:absolute;border:0;width:100vw;height:100vh}}</style></head><body><script>window.layoutPreview=()=>{{const f=document.getElementById('preview');if(f&&window.previewFrame)Object.assign(f.style,window.previewFrame)}};document.addEventListener('DOMContentLoaded',window.layoutPreview)</script><iframe id=preview sandbox=\"allow-scripts\" srcdoc=\"{escaped}\"></iframe></body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[test]
    fn confines_interactive_content() {
        let html = document("</iframe><script>parent.postMessage('test','*')</script>");
        assert_eq!(html.matches("<iframe ").count(), 1);
        assert_eq!(html.matches("</iframe>").count(), 1);
        assert!(html.contains("sandbox=\"allow-scripts\""));
        assert!(!html.contains("allow-same-origin"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("connect-src 'none'"));
    }
}
