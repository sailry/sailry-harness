//! GPUI's normal macOS window has no tracking area. Once a native child owns
//! the responder, AppKit can stop delivering mouse moves to GPUI. Kit b79f4ce
//! has no child-view event adapter; share one parent tracking area per window.
use objc2::{AnyThread, rc::Retained};
use objc2_app_kit::{NSTrackingArea, NSTrackingAreaOptions, NSView};
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::{Rc, Weak},
};
use wry::WebViewExtMacOS;

thread_local! {
    static PARENTS: RefCell<HashMap<usize, Weak<Pointer>>> = RefCell::default();
}

pub(super) struct Pointer {
    parent: Retained<NSView>,
    area: Retained<NSTrackingArea>,
}

impl Pointer {
    pub(super) fn attach(page: &wry::WebView) -> Rc<Self> {
        let parent = unsafe { page.webview().superview() }.expect("preview has a native parent");
        let key = Retained::as_ptr(&parent) as usize;
        PARENTS.with(|parents| {
            let mut parents = parents.borrow_mut();
            if let Some(existing) = parents.get(&key).and_then(Weak::upgrade) {
                return existing;
            }
            parents.retain(|_, value| value.strong_count() != 0);
            let area = unsafe {
                NSTrackingArea::initWithRect_options_owner_userInfo(
                    NSTrackingArea::alloc(),
                    parent.bounds(),
                    NSTrackingAreaOptions::MouseMoved
                        | NSTrackingAreaOptions::MouseEnteredAndExited
                        | NSTrackingAreaOptions::ActiveInKeyWindow
                        | NSTrackingAreaOptions::InVisibleRect,
                    Some(&parent),
                    None,
                )
            };
            parent.addTrackingArea(&area);
            let pointer = Rc::new(Self { parent, area });
            parents.insert(key, Rc::downgrade(&pointer));
            pointer
        })
    }
}

impl Drop for Pointer {
    fn drop(&mut self) {
        self.parent.removeTrackingArea(&self.area);
    }
}

pub(super) fn release_focus(page: &wry::WebView) {
    let view = page.webview();
    if let Some(responder) = view.window().and_then(|window| window.firstResponder())
        && let Some(focused) = responder.downcast_ref::<NSView>()
        && focused.isDescendantOf(&view)
    {
        let _ = page.focus_parent();
    }
}
