//! lb-wry 0.53.3 reports commit/finish but omits provisional start and failure.
//! Observe those main-frame events and forward every other selector to Wry, so
//! policy, script injection, downloads and completion keep their original owner.
use super::native::Event;
use objc2::{
    DeclaredClass, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject, Sel},
};
use objc2_foundation::NSError;
use objc2_web_kit::{WKNavigation, WKNavigationDelegate, WKWebView};
use std::cell::RefCell;

pub(super) struct State {
    delegate: Retained<ProtocolObject<dyn WKNavigationDelegate>>,
    current: RefCell<Option<Retained<WKNavigation>>>,
    events: tokio::sync::mpsc::UnboundedSender<(usize, Event)>,
    tab: usize,
}

define_class!(
    // SAFETY: NSObject has no subclass requirements; Observer does not implement Drop.
    #[unsafe(super(NSObject))]
    #[name = "SailryBrowserNavigationObserver"]
    #[thread_kind = MainThreadOnly]
    #[ivars = State]
    pub(super) struct Observer;

    // SAFETY: Observer inherits NSObject's required methods and preserves their signatures.
    unsafe impl NSObjectProtocol for Observer {
        // SAFETY: this override has NSObject's SEL argument and BOOL return signature.
        #[unsafe(method(respondsToSelector:))]
        fn responds(&self, selector: Sel) -> bool {
            // SAFETY: the initialized NSObject superclass implements this exact selector.
            let own: bool = unsafe { msg_send![super(self), respondsToSelector: selector] };
            own || self.ivars().delegate.respondsToSelector(selector)
        }
    }

    impl Observer {
        // SAFETY: this matches NSObject's SEL-to-id forwarding method; State
        // retains the returned delegate for the entire forwarding invocation.
        #[unsafe(method(forwardingTargetForSelector:))]
        fn forward(&self, _selector: Sel) -> *mut AnyObject {
            Retained::as_ptr(&self.ivars().delegate).cast_mut().cast()
        }
    }

    // SAFETY: all protocol methods are optional; these callbacks match the
    // pinned WKNavigationDelegate signatures and run only on the main thread.
    unsafe impl WKNavigationDelegate for Observer {
        #[unsafe(method(webView:didStartProvisionalNavigation:))]
        fn started(&self, _view: &WKWebView, navigation: Option<&WKNavigation>) {
            *self.ivars().current.borrow_mut() = navigation.map(Retained::from);
            let _ = self.ivars().events.send((self.ivars().tab, Event::Navigating));
        }

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn provisional_failure(&self, _view: &WKWebView, navigation: Option<&WKNavigation>, error: &NSError) {
            self.failed(navigation, error);
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn committed_failure(&self, _view: &WKWebView, navigation: Option<&WKNavigation>, error: &NSError) {
            self.failed(navigation, error);
        }
    }
);

impl Observer {
    pub(super) fn attach(
        view: &WKWebView,
        tab: usize,
        events: tokio::sync::mpsc::UnboundedSender<(usize, Event)>,
    ) -> Retained<Self> {
        // SAFETY: the caller retains Wry's WKWebView on the GPUI main thread;
        // the getter returns a retained navigation delegate with its protocol type.
        let delegate =
            unsafe { view.navigationDelegate() }.expect("Wry installs a navigation delegate");
        let observer = view.mtm().alloc::<Self>().set_ivars(State {
            delegate,
            current: RefCell::new(None),
            events,
            tab,
        });
        // SAFETY: Observer is allocated, all ivars are set, and NSObject's init
        // returns the initialized object according to the init ownership family.
        let observer: Retained<Self> = unsafe { msg_send![super(observer), init] };
        // SAFETY: observer implements WKNavigationDelegate and the caller keeps
        // the returned Retained observer while WebKit holds its weak delegate reference.
        unsafe { view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*observer))) };
        observer
    }

    fn failed(&self, navigation: Option<&WKNavigation>, error: &NSError) {
        // Replacing a navigation can cancel its predecessor after the new one
        // starts. Its cancellation must not clear the new page's spinner.
        if let Some(current) = self.ivars().current.borrow().as_deref()
            && navigation.is_some_and(|navigation| !std::ptr::eq(current, navigation))
        {
            return;
        }
        let cancelled = error.domain().to_string() == "NSURLErrorDomain" && error.code() == -999;
        if !cancelled {
            eprintln!(
                "sailry-desktop: browser navigation failed: {} ({})",
                error.domain(),
                error.code()
            );
        }
        let _ = self
            .ivars()
            .events
            .send((self.ivars().tab, Event::Failed { cancelled }));
    }
}
