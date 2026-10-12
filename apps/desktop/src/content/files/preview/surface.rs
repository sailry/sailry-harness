use super::*;
#[cfg(target_os = "windows")]
use crate::ui::webview_snapshot::Snapshot;
#[cfg(test)]
#[path = "overlay_tests.rs"]
mod overlays;

// Native child views bypass Kit's hitbox and paint order. Leave its divider's
// right-hand grab band clear: pinned Kit uses 4px padding plus a 1px hairline.
const RESIZE_CLEARANCE: Pixels = px(5.);

fn viewport(bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    let inset = RESIZE_CLEARANCE.min(bounds.size.width.max(px(0.)));
    Bounds::new(
        point(bounds.left() + inset, bounds.top()),
        size(bounds.size.width - inset, bounds.size.height),
    )
}

pub(crate) struct Native {
    pub(crate) page: gpui_webview::WebView,
    pub(super) frame: Cell<Option<(Bounds<Pixels>, Bounds<Pixels>)>>,
    scale: Cell<f32>,
    #[cfg(target_os = "windows")]
    snapshot: Rc<Snapshot>,
}
impl Native {
    pub(crate) fn new(html: &str, window: &mut Window, cx: &mut App) -> Result<Self, ()> {
        #[cfg(target_os = "windows")]
        let snapshot = Snapshot::new();
        #[cfg(target_os = "windows")]
        let signals = snapshot.signals();
        let builder = wry::WebViewBuilder::new()
            .with_html(document(html, cx))
            .with_ipc_handler(move |_request| {
                #[cfg(target_os = "windows")]
                signals.receive(_request.body());
            })
            .with_navigation_handler(|url| url == "about:blank" || url == "about:srcdoc");
        #[cfg(target_os = "windows")]
        let native = Self::build(builder, window, cx, snapshot);
        #[cfg(not(target_os = "windows"))]
        let native = Self::build(builder, window, cx);
        native
    }

    pub(crate) fn build(
        builder: wry::WebViewBuilder<'_>,
        window: &mut Window,
        cx: &mut App,
        #[cfg(target_os = "windows")] snapshot: Rc<Snapshot>,
    ) -> Result<Self, ()> {
        #[cfg(target_os = "windows")]
        let builder = snapshot.signals().prepare(builder);
        #[cfg(all(feature = "workload-tests", target_os = "macos"))]
        let builder = super::check::prepare(builder);
        let page = gpui_webview::WebView::build_hidden(
            builder
                .with_incognito(true)
                .with_focused(false)
                .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
                .with_download_started_handler(|_, _| false),
            window,
            cx,
        )
        .map_err(|_| ())?;
        Ok(Self {
            page,
            frame: Cell::new(None),
            scale: Cell::new(window.scale_factor()),
            #[cfg(target_os = "windows")]
            snapshot,
        })
    }
    fn show(&self, visible: bool) {
        if self.page.visible() != visible {
            let _ = self.page.set_visible(visible);
        }
    }
    fn hide(&self) {
        self.show(false);
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        self.snapshot.unmount();
        self.hide();
    }
}

struct Mount(Rc<Native>);
impl Drop for Mount {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        self.0.snapshot.unmount();
        self.0.hide();
    }
}

pub(crate) fn render(native: Rc<Native>, window: &mut Window, cx: &mut App) -> AnyElement {
    #[cfg(target_os = "windows")]
    native.snapshot.bind(window, cx);
    let mounted = native.clone();
    // Kit's managed WebView owns composition. Its element does not apply the
    // content mask or retained-entity unmount visibility, so this adapter only
    // supplies clipped bounds and frame lifetime through the managed API.
    let key: SharedString = format!("file-preview-mount-{:p}", Rc::as_ptr(&native)).into();
    window.use_keyed_state(key, cx, move |_, _| Mount(mounted));
    canvas(|bounds, window, _| window.insert_hitbox(viewport(bounds), HitboxBehavior::Normal), move |bounds, _hitbox, window, _cx| {
        let bounds = viewport(bounds);
        let clip = bounds.intersect(&window.content_mask().bounds);
        if clip.size.width <= px(0.) || clip.size.height <= px(0.) {
            #[cfg(target_os = "windows")]
            native.snapshot.unmount();
            native.hide(); return;
        }
        let previous = native.frame.get();
        let scale = window.scale_factor();
        native.page.set_scale_factor(scale);
        let scaled = native.scale.replace(scale) != scale;
        if scaled || previous.is_none_or(|(_, previous_clip)| previous_clip != clip) {
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
        #[cfg(target_os = "windows")]
        {
            let blocked = window.is_hitbox_occluded(&_hitbox);
            native.snapshot.mount((bounds, clip));
            native.snapshot.request(|| crate::ui::webview_capture::snapshot(native.page.raw()), window, _cx);
            let substituted = blocked && native.snapshot.paint(window);
            native.snapshot.shown.set(substituted);
            if substituted {
                native.hide();
                return;
            }
        }
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
        "<!doctype html><html><head><meta charset=utf-8><meta http-equiv=\"Content-Security-Policy\" content=\"{POLICY}; frame-src about:\"><style>html,body{{margin:0;overflow:hidden;background:{background};color:{foreground}}}iframe{{position:absolute;border:0;width:100vw;height:100vh}}</style></head><body><script>window.layoutPreview=()=>{{const f=document.getElementById('preview');if(f&&window.previewFrame)Object.assign(f.style,window.previewFrame)}};document.addEventListener('DOMContentLoaded',window.layoutPreview)</script><iframe id=preview sandbox=\"allow-scripts\" allow=\"camera 'none'; microphone 'none'; geolocation 'none'; clipboard-read 'none'\" referrerpolicy=\"no-referrer\" srcdoc=\"{escaped}\"></iframe></body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[test]
    fn leaves_resize_band_outside_native_view() {
        let bounds = Bounds::new(point(px(300.), px(48.)), size(px(640.), px(720.)));
        let native = viewport(bounds);
        assert_eq!(native.left(), bounds.left() + px(5.));
        assert_eq!(native.right(), bounds.right());
        assert_eq!(native.size.height, bounds.size.height);
        assert_eq!(layout(native, native), (native.size, point(px(0.), px(0.))));
        for width in [0., 3., 5.] {
            let narrow = viewport(Bounds::new(bounds.origin, size(px(width), px(720.))));
            assert_eq!(narrow.size.width, px(0.));
            assert!(narrow.left() <= bounds.left() + px(width));
        }
    }

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
