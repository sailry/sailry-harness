//! One local hidden surface prepares SVG; conversation rows remain native GPUI.
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native;

pub(crate) fn init(cx: &mut gpui_kit::App) {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    native::init(cx);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = cx;
}

pub(crate) fn release(window: &mut gpui_kit::Window, cx: &mut gpui_kit::App) {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    native::release(window, cx);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = (window, cx);
}
