//! Kit exposes no docked inspector. Reuse its managed native parent so WebKit
//! owns the bottom split without detaching the composition surface.
//! WebKit _WKInspector::attach and WebInspectorUIProxyMac own its split/resize UI.
use super::*;
use objc2::{rc::Retained, runtime::AnyObject};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView};
use std::rc::Rc;
use wry::WebViewExtMacOS as _;

mod delegate;
actions!(browser, [Inspect]);

pub(super) struct Surface {
    _container: Retained<NSView>,
    page: gpui_webview::WebViewHandle,
    _delegate: Retained<delegate::Delegate>,
    frame: Cell<Option<(Bounds<Pixels>, f32)>>,
}

impl Drop for Surface {
    fn drop(&mut self) {
        self.page.raw().close_devtools();
    }
}

impl Surface {
    fn new(page: &gpui_webview::WebView) -> Rc<Self> {
        let view = page.raw().webview();
        // SAFETY: browser actions run on the main thread and retain all views.
        // Wry's _inspector returns _WKInspector; setDelegate: takes an object
        // implementing its callback signatures, retained by Surface below.
        unsafe {
            // Kit owns this composition parent and releases it with the WebView.
            // Reparenting WKWebView would detach it from native composition.
            let container = view.superview().expect("browser has a native parent");
            view.setFrame(container.bounds());
            view.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            let delegate = delegate::Delegate::new(view.clone().into_super());
            let inspector: Retained<AnyObject> = objc2::msg_send![&view, _inspector];
            let (): () = objc2::msg_send![&inspector, setDelegate: &*delegate];
            Rc::new(Self {
                _delegate: delegate,
                _container: container,
                page: page.handle(),
                frame: Cell::new(None),
            })
        }
    }

    pub(super) fn layout(&self, page: &gpui_webview::WebView, bounds: Bounds<Pixels>, scale: f32) {
        // set_bounds resets WKWebView's frame. Apply it only on host resize so
        // WebKit retains ownership of the document/inspector split between frames.
        if self.frame.replace(Some((bounds, scale))) != Some((bounds, scale)) {
            let _ = page.set_bounds(super::surface::rect(bounds));
        }
    }
}

impl Browser {
    pub(crate) fn inspect(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.selected) else {
            return;
        };
        let Some(page) = &tab.page else {
            return;
        };
        let page = page.read(cx);
        if page.raw().is_devtools_open() {
            page.raw().close_devtools();
        } else {
            tab.surface.get_or_insert_with(|| Surface::new(page));
            page.raw().open_devtools();
            // SAFETY: this UI action retains Wry's WKWebView on the main thread;
            // its _inspector getter has the same object return signature used by Wry.
            unsafe {
                let inspector: Retained<AnyObject> =
                    objc2::msg_send![&page.raw().webview(), _inspector];
                delegate::attach(&page.raw().webview(), &inspector);
            }
        }
        cx.notify();
    }
}
