use super::*;
use crate::ui::webview_snapshot::Snapshot;
#[cfg(test)]
#[path = "overlay_tests.rs"]
mod overlays;

pub(crate) struct Native {
    pub(crate) page: wry::WebView,
    pub(super) frame: Cell<Option<(Bounds<Pixels>, Bounds<Pixels>)>>,
    visible: Cell<bool>,
    snapshot: Rc<Snapshot>,
    #[cfg(target_os = "macos")]
    _pointer: Rc<pointer::Pointer>,
}
impl Native {
    pub(super) fn new(html: &str, window: &Window) -> Result<Self, ()> {
        let snapshot = Snapshot::new();
        let signals = snapshot.signals();
        Self::build(
            wry::WebViewBuilder::new()
                .with_html(document(html))
                .with_ipc_handler(move |request| {
                    signals.receive(request.body());
                })
                .with_navigation_handler(|url| url == "about:blank" || url == "about:srcdoc"),
            window,
            snapshot,
        )
    }

    pub(crate) fn build(
        builder: wry::WebViewBuilder<'_>,
        window: &Window,
        snapshot: Rc<Snapshot>,
    ) -> Result<Self, ()> {
        let builder = snapshot.signals().prepare(builder);
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
            _pointer: pointer::Pointer::attach(&page)?,
            page,
            frame: Cell::new(None),
            visible: Cell::new(false),
            snapshot,
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
    #[cfg(all(feature = "workload-tests", target_os = "macos"))]
    pub(crate) fn substituted(&self) -> bool {
        self.snapshot.shown.get()
    }
    #[cfg(all(feature = "workload-tests", target_os = "macos"))]
    pub(crate) fn cache_state(&self) -> (u64, bool) {
        (self.snapshot.generation(), self.snapshot.current())
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        self.snapshot.unmount();
        self.hide();
    }
}

struct Mount(Rc<Native>);
impl Drop for Mount {
    fn drop(&mut self) {
        self.0.snapshot.unmount();
        self.0.hide();
    }
}

pub(crate) fn render(native: Rc<Native>, window: &mut Window, cx: &mut App) -> AnyElement {
    native.snapshot.bind(window, cx);
    let mounted = native.clone();
    // Kit element state lasts through cached frames and is released on unmount,
    // even when the owning session retains its preview entity in the background.
    let key: SharedString = format!("file-preview-mount-{:p}", Rc::as_ptr(&native)).into();
    window.use_keyed_state(key, cx, move |_, _| Mount(mounted));
    canvas(|bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal), move |bounds, hitbox, window, cx| {
        let blocked = window.is_hitbox_occluded(&hitbox);
        let clip = bounds.intersect(&window.content_mask().bounds);
        if clip.size.width <= px(0.) || clip.size.height <= px(0.) {
            native.snapshot.unmount();
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
        native.snapshot.mount((bounds, clip));
        #[cfg(target_os = "macos")]
        native.snapshot.refresh(blocked, || crate::ui::webview_capture::snapshot_now(&native.page), window);
        native.snapshot.request(|| crate::ui::webview_capture::snapshot(&native.page), window, cx);
        let substituted = blocked && native.snapshot.paint(window);
        native.snapshot.shown.set(substituted);
        if substituted {
            native.hide();
            return;
        }
        // Retain the live content until its first snapshot is ready, including
        // capture failures, so an overlapping surface never empties the preview.
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

    struct Overlap {
        bounds: Bounds<Pixels>,
        mask: Option<Bounds<Pixels>>,
        behavior: HitboxBehavior,
        result: Rc<Cell<bool>>,
    }

    impl Render for Overlap {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let result = self.result.clone();
            let bounds = self.bounds;
            let mask = self.mask;
            let behavior = self.behavior;
            div()
                .size_full()
                .child(
                    canvas(
                        |_, window, _| {
                            window.insert_hitbox(
                                Bounds::new(point(px(0.), px(0.)), size(px(100.), px(100.))),
                                HitboxBehavior::Normal,
                            )
                        },
                        move |_, hitbox, window, _| result.set(window.is_hitbox_occluded(&hitbox)),
                    )
                    .size_full(),
                )
                .child(deferred(
                    canvas(
                        move |_, window, _| {
                            window.with_content_mask(
                                mask.map(|bounds| ContentMask { bounds }),
                                |window| {
                                    window.insert_hitbox(bounds, behavior);
                                },
                            );
                        },
                        |_, _, _, _| {},
                    )
                    .size_full(),
                ))
        }
    }

    #[gpui::test]
    fn detects_only_visible_overlaps(cx: &mut TestAppContext) {
        let result = Rc::new(Cell::new(false));
        let (view, visual) = cx.add_window_view(|_, _| Overlap {
            bounds: Bounds::new(point(px(20.), px(20.)), size(px(40.), px(40.))),
            mask: None,
            behavior: HitboxBehavior::BlockMouse,
            result: result.clone(),
        });
        for (left, behavior, mask, expected) in [
            (20., HitboxBehavior::BlockMouse, None, true),
            (20., HitboxBehavior::BlockMouseExceptScroll, None, true),
            (20., HitboxBehavior::Normal, None, false),
            (100., HitboxBehavior::BlockMouse, None, false),
            (120., HitboxBehavior::BlockMouse, None, false),
            (
                80.,
                HitboxBehavior::BlockMouse,
                Some(Bounds::new(
                    point(px(100.), px(0.)),
                    size(px(40.), px(100.)),
                )),
                false,
            ),
        ] {
            view.update(visual, |view, cx| {
                view.bounds.origin.x = px(left);
                view.behavior = behavior;
                view.mask = mask;
                cx.notify();
            });
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert_eq!(result.get(), expected);
        }
        visual.update(|window, _| window.remove_window());
    }
}
