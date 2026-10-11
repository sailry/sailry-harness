//! Observe native keyboard ownership while hidden browser tools run.
use super::*;
use gpui_kit::component::WindowExt as _;
#[cfg(target_os = "macos")]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub(super) struct State {
    gpui: Option<FocusHandle>,
    native: usize,
    #[cfg(target_os = "macos")]
    _responder: objc2::rc::Retained<objc2_app_kit::NSResponder>,
}

impl State {
    pub(super) async fn foreground(
        browser: &Entity<Browser>,
        selected: u64,
        cx: &mut AsyncWindowContext,
    ) -> Self {
        cx.update(|window, _| window.activate_window()).unwrap();
        cx.update(|window, cx| window.clear_notifications(cx))
            .unwrap();
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let mut previous = None;
        until(cx, "foreground native browser", |_window, cx| {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            {
                let mut state = browser.read(cx).visibility_state(selected, cx);
                state["active_dialog"] = _window.has_active_dialog(cx).into();
                if previous.as_ref() != Some(&state) {
                    println!("Browser native visibility: {state}");
                    previous = Some(state);
                }
            }
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            {
                browser
                    .read(cx)
                    .handle(selected, cx)
                    .is_some_and(|page| visible(page.raw()))
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            false
        })
        .await;
        cx.update(|window, cx| {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            {
                let page = browser.read(cx).handle(selected, cx).unwrap();
                page.raw().focus().unwrap();
                assert!(
                    focused(page.raw()),
                    "the retained foreground WebView must own native keyboard focus"
                );
            }
            Self::capture(window, cx)
        })
        .unwrap()
    }

    pub(super) fn composer(
        shell: &Entity<Shell>,
        browser: &Entity<Browser>,
        selected: u64,
        cx: &mut AsyncWindowContext,
    ) -> Self {
        cx.update(|window, cx| {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            browser
                .read(cx)
                .handle(selected, cx)
                .unwrap()
                .raw()
                .focus_parent()
                .unwrap();
            // Establish both the host's native responder and the real composer
            // focus before the next background call; no restoration runs later.
            let source = shell.read(cx).current_chat().cloned().unwrap();
            source.update(cx, |source, cx| source.focus(window, cx));
            let state = Self::capture(window, cx);
            assert!(state.gpui.is_some(), "the composer must own GPUI focus");
            state
        })
        .unwrap()
    }

    fn capture(window: &Window, cx: &App) -> Self {
        #[cfg(target_os = "macos")]
        {
            let handle = HasWindowHandle::window_handle(window).unwrap();
            let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
                panic!("AppKit window expected")
            };
            // SAFETY: the workload runs on GPUI's AppKit thread, and the live
            // window owns this native content view and its responder.
            let view = unsafe { handle.ns_view.cast::<objc2_app_kit::NSView>().as_ref() };
            let responder = view.window().unwrap().firstResponder().unwrap();
            Self {
                gpui: window.focused(cx),
                native: objc2::rc::Retained::as_ptr(&responder) as usize,
                _responder: responder,
            }
        }
        #[cfg(target_os = "windows")]
        {
            // SAFETY: the workload runs on the window's UI thread; GetFocus
            // reports the actual receiving HWND without changing it.
            let native = unsafe { windows_webview::Win32::UI::Input::KeyboardAndMouse::GetFocus() };
            assert!(!native.0.is_null(), "a native focus HWND is required");
            Self {
                gpui: window.focused(cx),
                native: native.0 as usize,
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (window, cx);
            panic!("native focus workload requires macOS or Windows")
        }
    }

    pub(super) fn check(
        &self,
        browser: &Entity<Browser>,
        tab: Option<u64>,
        stage: &str,
        cx: &mut AsyncWindowContext,
    ) {
        cx.update(|window, cx| {
            let current = Self::capture(window, cx);
            assert_eq!(self.gpui, current.gpui, "{stage}: GPUI focus changed");
            assert_eq!(
                self.native, current.native,
                "{stage}: native keyboard responder changed"
            );
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            if let Some(tab) = tab {
                let page = browser.read(cx).handle(tab, cx).unwrap();
                assert!(!visible(page.raw()), "{stage}: background child is visible");
            }
            println!(
                "Browser native focus: {stage}, responder={}, unchanged=true",
                self.native
            );
        })
        .unwrap();
    }
}

#[cfg(target_os = "macos")]
fn visible(page: &wry::WebView) -> bool {
    use wry::WebViewExtMacOS as _;
    !page.webview().isHidden()
}

#[cfg(target_os = "windows")]
fn visible(page: &wry::WebView) -> bool {
    use wry::WebViewExtWindows as _;
    let mut visible = windows_webview::core::BOOL::default();
    // SAFETY: the retained controller and output pointer are valid on this UI thread.
    unsafe { page.controller().IsVisible(&mut visible).unwrap() };
    visible.as_bool()
}

#[cfg(target_os = "macos")]
fn focused(page: &wry::WebView) -> bool {
    use wry::WebViewExtMacOS as _;
    let view = page.webview();
    view.window()
        .and_then(|window| window.firstResponder())
        .is_some_and(|responder| {
            responder
                .downcast_ref::<objc2_app_kit::NSView>()
                .is_some_and(|responder| responder.isDescendantOf(&view))
        })
}

#[cfg(target_os = "windows")]
fn focused(page: &wry::WebView) -> bool {
    use windows_webview::Win32::{
        Foundation::HWND,
        UI::{Input::KeyboardAndMouse::GetFocus, WindowsAndMessaging::IsChild},
    };
    use wry::WebViewExtWindows as _;
    let mut parent = HWND::default();
    // SAFETY: controller queries and focus inspection run on its owning UI thread.
    unsafe {
        page.controller().ParentWindow(&mut parent).unwrap();
        let focus = GetFocus();
        focus != parent && IsChild(parent, focus).as_bool()
    }
}
