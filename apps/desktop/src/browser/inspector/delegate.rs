//! WebKit creates its inspector asynchronously; attach only after the frontend loads.
use objc2::{
    DeclaredClass, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObject, NSObjectProtocol},
};
use objc2_web_kit::WKWebView;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "SailryWebInspectorDelegate"]
    #[thread_kind = MainThreadOnly]
    #[ivars = Retained<WKWebView>]
    pub(super) struct Delegate;
    unsafe impl NSObjectProtocol for Delegate {}
    impl Delegate {
        #[unsafe(method(inspectorFrontendLoaded:))]
        fn loaded(&self, inspector: &AnyObject) {
            unsafe { let (): () = msg_send![inspector, show]; }
            attach(self.ivars(), inspector);
        }
    }
);

impl Delegate {
    pub(super) fn new(view: Retained<WKWebView>) -> Retained<Self> {
        let this = Self::alloc(view.mtm()).set_ivars(view);
        unsafe { msg_send![super(this), init] }
    }
}

pub(super) fn attach(view: &WKWebView, inspector: &AnyObject) {
    unsafe {
        let parent = view.superview().expect("inspector has a browser container");
        let bounds = parent.bounds();
        // WebKit refuses the initial dock below 500x334, but supports resizing an
        // attached inspector smaller. Bootstrap docking before restoring the real
        // sidebar dimensions, within the same native event callback.
        let mut frame = bounds;
        frame.size.width = frame.size.width.max(500.);
        frame.size.height = frame.size.height.max(334.);
        view.setFrame(frame);
        let (): () = msg_send![inspector, attach];
        let panel: Option<Retained<WKWebView>> = msg_send![inspector, inspectorWebView];
        if let Some(panel) = panel
            && panel.superview() == view.superview()
        {
            let mut frame = bounds;
            frame.size.height = (bounds.size.height * 0.4).min(300.);
            panel.setFrame(frame);
        }
        let mut frame = view.frame();
        frame.size.width = bounds.size.width;
        frame.origin.y = if frame.origin.y > 0. {
            (bounds.size.height * 0.4).min(300.)
        } else {
            0.
        };
        frame.size.height = bounds.size.height - frame.origin.y;
        view.setFrame(frame);
    }
}
