//! Stock Kit owns native composition, bounds, focus and hit testing.
use super::*;
use gpui_kit::prelude::FluentBuilder as _;
use std::rc::Rc;

// Kit's private resize band extends 4 px into each panel plus its 1 px seam.
// Windows native children sit above GPUI. Reserve its divider band instead of
// treating a permanent mouse blocker as an overlay over the whole page.
pub(super) const RESIZE_INSET: Pixels = px(5.);

struct Mount {
    mounted: Rc<Cell<bool>>,
}

impl Drop for Mount {
    fn drop(&mut self) {
        self.mounted.replace(false);
    }
}

pub(super) fn render(
    page: &Entity<gpui_webview::WebView>,
    mounted: &mut Rc<Cell<bool>>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
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
    active.set(true);
    page.update(cx, |page, _| page.show());
    div()
        .size_full()
        .when(cfg!(target_os = "windows"), |view| view.p(RESIZE_INSET))
        .track_focus(&focus)
        .child(page.clone())
        .into_any_element()
}
