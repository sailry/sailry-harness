//! Present retained UI error states without adding rows to the owning view.
use crate::tr;
use gpui_kit::component::{
    WindowExt,
    notification::{Notification, NotificationDelivery, NotificationType},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::HashSet;
/// Present operation feedback through Kit's transient toast layer.
pub(crate) fn toast(
    window: &mut Window,
    summary: SharedString,
    message: Notification,
    cx: &mut App,
) {
    deliver(window, message, cx);
    #[cfg(test)]
    tests::record(window, summary, cx);
    #[cfg(not(test))]
    let _ = summary;
}

pub(crate) fn status(
    window: &mut Window,
    summary: SharedString,
    _kind: NotificationType,
    message: Notification,
    cx: &mut App,
) {
    toast(window, summary, message, cx);
}

fn deliver(window: &mut Window, message: Notification, cx: &mut App) {
    window.push_notification(
        message
            .min_w(minimum_width(window))
            .delivery(NotificationDelivery::InApp)
            .placement(Anchor::TopCenter),
        cx,
    );
}

pub(crate) fn minimum_width(window: &Window) -> Pixels {
    px(240.).min((window.viewport_size().width - px(48.)).max(px(0.)))
}

pub(crate) fn info(title: &str, detail: &str, window: &mut Window, cx: &mut App) {
    deliver(
        window,
        Notification::info(detail.to_owned())
            .id1::<Info>(SharedString::from(format!(
                "{}:{title}{detail}",
                title.len()
            )))
            .when(!title.trim().is_empty(), |note| {
                note.title(title.to_owned())
            }),
        cx,
    );
    #[cfg(test)]
    tests::record(window, detail.to_owned().into(), cx);
}

struct Info;

pub(crate) fn error(title: &str, detail: &str, window: &mut Window, cx: &mut App) {
    deliver(
        window,
        diagnostic(detail.to_owned().into()).when(!title.trim().is_empty(), |note| {
            note.title(title.to_owned())
        }),
        cx,
    );
    #[cfg(test)]
    tests::record(window, detail.to_owned().into(), cx);
}

pub(crate) fn diagnostic(message: SharedString) -> Notification {
    Notification::new()
        .with_type(NotificationType::Error)
        .content(move |_, _, _| {
            div()
                .id("error-detail")
                .debug_selector(|| "error-toast-detail".into())
                .max_h_24()
                .overflow_y_scroll()
                .text_sm()
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .child(message.clone())
                .into_any_element()
        })
}

/// Observe asynchronous form and panel errors; ordinary redraws do not repeat toasts.
/// Error state remains with its owner for retry and revision guards.
pub(crate) fn observe<T: 'static>(
    window: &Window,
    cx: &mut Context<T>,
    errors: impl Fn(&T, &App) -> Vec<&'static str> + 'static,
) {
    observe_with(window, cx, errors, |_, key, _| Notification::error(tr(key)));
}

pub(crate) fn observe_with<T: 'static, Errors>(
    window: &Window,
    cx: &mut Context<T>,
    errors: Errors,
    build: impl Fn(&T, &'static str, &mut Context<T>) -> Notification + 'static,
) where
    Errors: Fn(&T, &App) -> Vec<&'static str> + 'static,
{
    let window = window.window_handle();
    let mut previous = HashSet::new();
    cx.observe_self(move |view, cx| {
        let current: HashSet<_> = errors(view, cx).into_iter().collect();
        let added: Vec<_> = current.difference(&previous).copied().collect();
        let cleared: Vec<_> = previous.difference(&current).copied().collect();
        previous = current;
        let owner = cx.entity_id();
        if !added.is_empty() || !cleared.is_empty() {
            let messages: Vec<_> = added
                .into_iter()
                .map(|key| (tr(key), build(view, key, cx).id1::<Errors>((key, owner))))
                .collect();
            _ = window.update(cx, |_, window, cx| {
                for key in cleared {
                    window.remove_notification1::<Errors>((key, owner), cx);
                }
                for (summary, message) in messages {
                    toast(window, summary, message, cx);
                }
            });
        }
    })
    .detach();
    let owner = cx.weak_entity();
    cx.defer(move |cx| {
        _ = owner.update(cx, |_, cx| cx.notify());
    });
}

#[cfg(test)]
pub(crate) mod tests;
