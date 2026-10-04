use gpui_kit::Window;
use objc2::MainThreadOnly;
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
    NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub fn set_min_size(window: &Window, size: gpui_kit::Size<gpui_kit::Pixels>) {
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    // GPUI owns the live content view and invokes rendering on the AppKit thread.
    let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    if let Some(window) = view.window() {
        window.setContentMinSize(objc2_foundation::NSSize::new(
            f32::from(size.width) as f64,
            f32::from(size.height) as f64,
        ));
    }
}

pub fn configure_backdrop(window: &Window) {
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };

    // GPUI owns this NSView and calls us on the AppKit main thread while the window is live.
    let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    let Some(content) = view.window().and_then(|window| window.contentView()) else {
        return;
    };
    for child in content.subviews() {
        if let Some(effect) = child.downcast_ref::<NSVisualEffectView>() {
            // Preserve GPUI's ownership, but bypass its subclass that strips native material colors.
            effect.setHidden(true);
        }
    }

    let effect =
        NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(view.mtm()), content.bounds());
    effect.setMaterial(NSVisualEffectMaterial::Sidebar);
    effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    effect.setState(NSVisualEffectState::Active);
    effect.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    // AppKit retains this view and inherits the appearance set by the global theme observer.
    content.addSubview_positioned_relativeTo(&effect, NSWindowOrderingMode::Below, None);
}
