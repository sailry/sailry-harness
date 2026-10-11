//! Kit owns native composition. Supply clipped bounds and retained-tab mount
//! visibility; Windows also retains its native-child overlay fallback.
use super::*;
#[cfg(target_os = "windows")]
use crate::ui::webview_snapshot::Snapshot;
use gpui_kit::prelude::FluentBuilder as _;
use std::rc::Rc;

// Kit's private resize band extends 4 px into each panel plus its 1 px seam.
// Windows native children sit above GPUI. Reserve its divider band instead of
// treating a permanent mouse blocker as an overlay over the whole page.
pub(super) const RESIZE_INSET: Pixels = px(5.);

struct Mount {
    #[cfg(target_os = "windows")]
    snapshot: Rc<Snapshot>,
    mounted: Rc<Cell<bool>>,
}

impl Drop for Mount {
    fn drop(&mut self) {
        if !self.mounted.replace(false) {
            return;
        }
        #[cfg(target_os = "windows")]
        self.snapshot.unmount();
    }
}

pub(super) fn render(
    _tab: &Tab,
    page: &Entity<gpui_webview::WebView>,
    mounted: &mut Rc<Cell<bool>>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    #[cfg(target_os = "windows")]
    let snapshot = _tab.snapshot.clone();
    #[cfg(target_os = "windows")]
    snapshot.bind(window, cx);
    let raw = page.read(cx).handle();
    #[cfg(target_os = "windows")]
    let retained_snapshot = snapshot.clone();
    let retained_entity = page.downgrade();
    let key: SharedString = format!("browser-mount-{}", page.entity_id()).into();
    // Existing keyed state may discard the initializer without calling it.
    let mount = window.use_keyed_state(key, cx, move |_, cx| {
        // GPUI releases listeners before dropping the state. Keep Kit's flag and
        // native focus coherent; inactive replacements own their own cleanup.
        cx.on_release(move |mount: &mut Mount, cx| {
            if mount.mounted.get()
                && let Some(page) = retained_entity.upgrade()
            {
                page.update(cx, |page, _| page.hide());
            }
        })
        .detach();
        Mount {
            #[cfg(target_os = "windows")]
            snapshot: retained_snapshot,
            mounted: Rc::new(Cell::new(false)),
        }
    });
    let active = mount.read(cx).mounted.clone();
    if !Rc::ptr_eq(mounted, &active) {
        // An outgoing tab's later Drop must not clear the incoming mount or focus.
        mounted.set(false);
        *mounted = active.clone();
    }
    let focus = page.read(cx).focus_handle(cx);
    let page = page.clone();
    #[cfg(target_os = "macos")]
    let inspector = _tab.surface.clone();
    div()
        .size_full()
        .when(cfg!(target_os = "windows"), |view| view.p(RESIZE_INSET))
        .track_focus(&focus)
        .child(
            canvas(
                |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                move |bounds, _hitbox, window, cx| {
                    let clip = bounds.intersect(&window.content_mask().bounds);
                    if clip.size.width <= px(0.) || clip.size.height <= px(0.) {
                        active.set(false);
                        #[cfg(target_os = "windows")]
                        snapshot.unmount();
                        page.update(cx, |page, _| page.hide());
                        return;
                    }
                    active.set(true);
                    page.update(cx, |page, _cx| {
                        page.set_scale_factor(window.scale_factor());
                        #[cfg(target_os = "macos")]
                        if let Some(surface) = &inspector {
                            surface.layout(page, clip, window.scale_factor());
                        } else {
                            let _ = page.set_bounds(rect(clip));
                        }
                        #[cfg(not(target_os = "macos"))]
                        let _ = page.set_bounds(rect(clip));
                        #[cfg(target_os = "windows")]
                        {
                            let blocked = window.is_hitbox_occluded(&_hitbox);
                            snapshot.mount((clip, clip));
                            snapshot.request(
                                || crate::ui::webview_capture::snapshot(page.raw()),
                                window,
                                _cx,
                            );
                            let substituted = blocked && snapshot.paint(window);
                            snapshot.shown.set(substituted);
                            if substituted {
                                page.hide();
                                return;
                            }
                        }
                        page.show();
                    });
                    let raw = raw.clone();
                    window.on_mouse_event(move |event: &MouseDownEvent, phase, _, _| {
                        if phase == DispatchPhase::Capture && !clip.contains(&event.position) {
                            let _ = raw.raw().focus_parent();
                        }
                    });
                },
            )
            .size_full(),
        )
        .into_any_element()
}

pub(super) fn rect(bounds: Bounds<Pixels>) -> wry::Rect {
    wry::Rect {
        position: wry::dpi::LogicalPosition::new(
            f64::from(f32::from(bounds.left())),
            f64::from(f32::from(bounds.top())),
        )
        .into(),
        size: wry::dpi::LogicalSize::new(
            f64::from(f32::from(bounds.size.width)),
            f64::from(f32::from(bounds.size.height)),
        )
        .into(),
    }
}
