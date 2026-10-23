//! Application styles for Kit surfaces with public refinement hooks.
use gpui_kit::{
    component::{
        ActiveTheme as _, StyledExt as _, Theme, notification::Notification, popover::Popover,
        tooltip::Tooltip,
    },
    *,
};

/// Kit 7fc6a4c draws its default popover edge as a private shadow ring.
/// Public surface styles are merged afterward, so semantic elevation removes
/// that ring without changing the control, its motion, or dark appearance.
pub(crate) fn popover(surface: Popover, cx: &App) -> Popover {
    surface.refine_style(&popup_style(cx))
}

/// Shared by styled Popover and application-owned Base Popover content.
pub(crate) fn popup_style(cx: &App) -> StyleRefinement {
    if cx.theme().is_dark() {
        StyleRefinement::default()
    } else {
        StyleRefinement::default()
            .border_0()
            .shadow(cx.theme().shadow_tokens().md)
    }
}

pub(super) fn init(cx: &mut App) {
    // Both Kit views merge their public Styled border after the default.
    borders::<Tooltip>(cx);
    borders::<Notification>(cx);
}

fn borders<T: Styled + 'static>(cx: &mut App) {
    cx.observe_new::<T>(|surface, _, cx| {
        let borders = surface.style().border_widths.clone();
        surface.style().border_widths = if cx.theme().is_dark() {
            borders.clone()
        } else {
            StyleRefinement::default().border_0().border_widths
        };
        let observer = cx.observe_global::<Theme>(move |surface, cx| {
            surface.style().border_widths = if cx.theme().is_dark() {
                borders.clone()
            } else {
                StyleRefinement::default().border_0().border_widths
            };
            cx.notify();
        });
        // Drop the subscription with the surface, including between theme changes.
        cx.on_release(move |_, _| drop(observer)).detach();
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::{component::ThemeMode, gpui};

    #[gpui::test]
    fn surfaces_restore_custom_borders(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            init(cx);
            Theme::change(ThemeMode::Light, None, cx);
        });
        let custom = cx.new(|_| Tooltip::new("Custom").border_2());
        let default = cx.new(|_| Tooltip::new("Default"));
        let toast = cx.new(|_| Notification::info("Saved").border_2());
        cx.update(|cx| {
            custom.update(cx, |tooltip, _| {
                assert_eq!(
                    tooltip.style().border_widths,
                    StyleRefinement::default().border_0().border_widths
                );
            });
            toast.update(cx, |toast, _| {
                assert_eq!(
                    toast.style().border_widths,
                    StyleRefinement::default().border_0().border_widths
                );
            });
            Theme::change(ThemeMode::Dark, None, cx);
        });
        cx.update(|cx| {
            custom.update(cx, |tooltip, _| {
                assert_eq!(
                    tooltip.style().border_widths,
                    StyleRefinement::default().border_2().border_widths
                );
            });
            default.update(cx, |tooltip, _| {
                assert_eq!(
                    tooltip.style().border_widths,
                    StyleRefinement::default().border_widths
                );
            });
            toast.update(cx, |toast, _| {
                assert_eq!(
                    toast.style().border_widths,
                    StyleRefinement::default().border_2().border_widths
                );
            });
            Theme::change(ThemeMode::Light, None, cx);
        });
        cx.update(|cx| {
            custom.update(cx, |tooltip, _| {
                assert_eq!(
                    tooltip.style().border_widths,
                    StyleRefinement::default().border_0().border_widths
                );
            });
            toast.update(cx, |toast, _| {
                assert_eq!(
                    toast.style().border_widths,
                    StyleRefinement::default().border_0().border_widths
                );
            });
        });
    }
}
