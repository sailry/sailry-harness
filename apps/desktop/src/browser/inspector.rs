//! Kit b79f4ce/Wry resize WKWebView directly and expose no docked inspector.
//! A native parent confines WebKit's own bottom split to this browser surface.
//! WebKit _WKInspector::attach and WebInspectorUIProxyMac own its split/resize UI.
use super::*;
use objc2::{MainThreadMarker, MainThreadOnly, rc::Retained, runtime::AnyObject};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::rc::Rc;
use wry::WebViewExtMacOS as _;

mod delegate;
actions!(browser, [Inspect]);

pub(super) struct Surface {
    container: Retained<NSView>,
    page: gpui_wry::WebViewHandle,
    _delegate: Retained<delegate::Delegate>,
}

impl Drop for Surface {
    fn drop(&mut self) {
        self.page.raw().close_devtools();
        self.container.removeFromSuperview();
    }
}

impl Surface {
    fn new(page: &gpui_wry::WebView) -> Rc<Self> {
        let view = page.raw().webview();
        let thread = MainThreadMarker::new().expect("browser runs on the UI thread");
        unsafe {
            let container = NSView::initWithFrame(NSView::alloc(thread), view.frame());
            let parent = view.superview().expect("browser has a native parent");
            parent.addSubview(&container);
            view.removeFromSuperview();
            container.addSubview(&view);
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
                container,
                page: page.handle(),
            })
        }
    }

    pub(super) fn visible(&self, visible: bool) {
        self.container.setHidden(!visible);
    }

    pub(super) fn element(
        self: &Rc<Self>,
        page: &Entity<gpui_wry::WebView>,
        cx: &App,
    ) -> AnyElement {
        let surface = self.clone();
        let raw = self.page.clone();
        div()
            .size_full()
            .track_focus(&page.read(cx).focus_handle(cx))
            .child(
                canvas(
                    move |bounds, _, _| unsafe {
                        let parent = surface
                            .container
                            .superview()
                            .expect("browser container has a parent");
                        let x = f64::from(f32::from(bounds.origin.x));
                        let y = f64::from(f32::from(bounds.origin.y));
                        let width = f64::from(f32::from(bounds.size.width));
                        let height = f64::from(f32::from(bounds.size.height));
                        let y = if parent.isFlipped() {
                            y
                        } else {
                            parent.bounds().size.height - y - height
                        };
                        let frame = NSRect::new(NSPoint::new(x, y), NSSize::new(width, height));
                        if surface.container.frame() != frame {
                            surface.container.setFrame(frame);
                        }
                    },
                    move |bounds, _, window, _| {
                        let raw = raw.clone();
                        window.on_mouse_event(move |event: &MouseDownEvent, _, _, _| {
                            if !bounds.contains(&event.position) {
                                let _ = raw.raw().focus_parent();
                            }
                        });
                    },
                )
                .size_full(),
            )
            .into_any_element()
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
            unsafe {
                let inspector: Retained<AnyObject> =
                    objc2::msg_send![&page.raw().webview(), _inspector];
                delegate::attach(&page.raw().webview(), &inspector);
            }
        }
        cx.notify();
    }
}
