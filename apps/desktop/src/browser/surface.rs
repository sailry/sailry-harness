//! Kit's Wry element has no native clipping or overlay substitution. Reuse the
//! preview cache while retaining browser focus, tabs and inspector ownership.
use super::*;
use crate::ui::webview_snapshot::Snapshot;
use std::rc::Rc;

// Kit's private resize band extends 4 px into each panel plus its 1 px seam.
// Native children sit above GPUI, so reserve that band for the divider instead
// of treating its permanent mouse blocker as an overlay over the whole page.
pub(super) const RESIZE_INSET: Pixels = px(5.);

struct Mount {
    page: gpui_wry::WebViewHandle,
    snapshot: Rc<Snapshot>,
    mounted: Rc<Cell<bool>>,
    #[cfg(target_os = "macos")]
    inspector: Option<Rc<inspector::Surface>>,
}

impl Drop for Mount {
    fn drop(&mut self) {
        if !self.mounted.replace(false) {
            return;
        }
        self.snapshot.unmount();
        let _ = self.page.raw().set_visible(false);
        #[cfg(target_os = "macos")]
        if let Some(surface) = &self.inspector {
            surface.visible(false);
        }
    }
}

pub(super) fn render(
    tab: &Tab,
    page: &Entity<gpui_wry::WebView>,
    mounted: &mut Rc<Cell<bool>>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let snapshot = tab.snapshot.clone();
    snapshot.bind(window, cx);
    let raw = page.read(cx).handle();
    let retained_page = raw.clone();
    let retained_snapshot = snapshot.clone();
    let retained_entity = page.downgrade();
    #[cfg(target_os = "macos")]
    let retained_inspector = tab.surface.clone();
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
            page: retained_page,
            snapshot: retained_snapshot,
            mounted: Rc::new(Cell::new(false)),
            #[cfg(target_os = "macos")]
            inspector: retained_inspector,
        }
    });
    let active = mount.read(cx).mounted.clone();
    if !Rc::ptr_eq(mounted, &active) {
        // An outgoing tab's later Drop must not clear the incoming mount or focus.
        mounted.set(false);
        *mounted = active.clone();
    }
    #[cfg(target_os = "macos")]
    mount.update(cx, |mount, _| mount.inspector = tab.surface.clone());
    #[cfg(not(target_os = "macos"))]
    let _ = mount;
    let focus = page.read(cx).focus_handle(cx);
    let page = page.clone();
    #[cfg(target_os = "macos")]
    let inspector = tab.surface.clone();
    div()
        .size_full()
        .p(RESIZE_INSET)
        .track_focus(&focus)
        .child(
            canvas(
                |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                move |bounds, hitbox, window, cx| {
                    let clip = bounds.intersect(&window.content_mask().bounds);
                    let blocked = window.is_hitbox_occluded(&hitbox);
                    if clip.size.width <= px(0.) || clip.size.height <= px(0.) {
                        active.set(false);
                        snapshot.unmount();
                        page.update(cx, |page, _| page.hide());
                        #[cfg(target_os = "macos")]
                        if let Some(surface) = &inspector {
                            surface.visible(false);
                        }
                        return;
                    }
                    active.set(true);
                    #[cfg(target_os = "macos")]
                    let painted = if let Some(surface) = &inspector {
                        surface.layout(clip)
                    } else {
                        clip
                    };
                    #[cfg(not(target_os = "macos"))]
                    let painted = clip;
                    #[cfg(target_os = "macos")]
                    let docked = inspector.is_some();
                    #[cfg(not(target_os = "macos"))]
                    let docked = false;
                    if !docked {
                        let _ = raw.raw().set_bounds(wry::Rect {
                            position: wry::dpi::LogicalPosition::new(
                                f64::from(f32::from(clip.left())),
                                f64::from(f32::from(clip.top())),
                            )
                            .into(),
                            size: wry::dpi::LogicalSize::new(
                                f64::from(f32::from(clip.size.width)),
                                f64::from(f32::from(clip.size.height)),
                            )
                            .into(),
                        });
                    }
                    snapshot.mount((painted, painted.intersect(&clip)));
                    #[cfg(target_os = "macos")]
                    snapshot.refresh(
                        blocked,
                        || crate::ui::webview_capture::snapshot_now(raw.raw()),
                        window,
                    );
                    snapshot.request(
                        || crate::ui::webview_capture::snapshot(raw.raw()),
                        window,
                        cx,
                    );
                    let substituted = blocked && snapshot.paint(window);
                    snapshot.shown.set(substituted);
                    page.update(cx, |page, _| {
                        if substituted {
                            if page.visible() {
                                page.hide();
                            }
                        } else {
                            page.show();
                        }
                    });
                    #[cfg(target_os = "macos")]
                    if let Some(surface) = &inspector {
                        surface.visible(!substituted);
                    }
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
