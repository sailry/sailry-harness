//! Native previews share one key-window tracking area on their parent view.
//! Each child retains the adapter; the last preview removes its tracking area.
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
    pub(super) fn attach(page: &wry::WebView) -> Result<Rc<Self>, ()> {
        // SAFETY: Wry retains the view, and preview mounting runs on AppKit's thread.
        let parent = unsafe { page.webview().superview() }.ok_or(())?;
        let key = Retained::as_ptr(&parent) as usize;
        Ok(PARENTS.with(|parents| {
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
        }))
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
