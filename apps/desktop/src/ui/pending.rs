//! Region loading overlay composed from Kit Spinner. It covers its nearest
//! positioned ancestor and fades in after a short delay, so fast reads show nothing.
use gpui_kit::{
    component::{ActiveTheme as _, Sizable as _, spinner::Spinner},
    *,
};
use std::time::Duration;

const FADE: Duration = Duration::from_millis(600);

/// Opacity for the animation progress: hidden for the first half, then fading in.
fn fade(delta: f32) -> f32 {
    ((delta - 0.5) * 2.0).clamp(0.0, 1.0)
}

#[cfg(test)]
thread_local! {
    pub(super) static LAST_OPACITY: std::cell::Cell<Option<f32>> = const { std::cell::Cell::new(None) };
}

/// A blocking overlay for a `relative()` region that has nothing to show yet.
pub(crate) fn overlay(id: impl Into<String>, cx: &App) -> AnyElement {
    let id: SharedString = id.into().into();
    let selector = id.clone();
    div()
        .id(id.clone())
        .debug_selector(move || selector.to_string())
        .aria_label(crate::tr("plugins_loading"))
        .absolute()
        .inset_0()
        .occlude()
        .bg(cx.theme().background.opacity(0.8))
        .flex()
        .items_center()
        .justify_center()
        .child(Spinner::new().large())
        .with_animation(id, Animation::new(FADE), |element, delta| {
            let opacity = fade(delta);
            #[cfg(test)]
            LAST_OPACITY.with(|last| last.set(Some(opacity)));
            element.opacity(opacity)
        })
        .into_any_element()
}

#[cfg(test)]
mod tests;
