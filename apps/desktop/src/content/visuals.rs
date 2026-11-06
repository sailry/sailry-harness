//! One local hidden surface prepares SVG; conversation rows remain native GPUI.
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native;
pub(crate) mod snapshot;

pub(crate) fn init(cx: &mut gpui_kit::App) {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    native::init(cx);
    snapshot::init(cx);
}

pub(crate) fn release(window: &mut gpui_kit::Window, cx: &mut gpui_kit::App) {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    native::release(window, cx);
    snapshot::release(window, cx);
}
