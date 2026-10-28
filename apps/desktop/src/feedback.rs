//! Present retained UI error states without adding rows to the owning view.
use crate::tr;
use gpui_kit::component::{
    WindowExt,
    notification::{Notification, NotificationDelivery, NotificationType},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::{cell::RefCell, collections::HashSet, rc::Rc};

/// Recovery controls belong to an active error; ordinary outcome toasts do not.
pub(crate) struct Feedback {
    message: Notification,
    scoped: bool,
}

impl From<Notification> for Feedback {
    fn from(message: Notification) -> Self {
        Self {
            message,
            scoped: false,
        }
    }
}

pub(crate) trait Recovery {
    fn scoped_action(
        self,
        action: impl Fn(
            &mut Notification,
            &mut Window,
            &mut Context<Notification>,
        ) -> component::button::Button
        + 'static,
    ) -> Feedback;
}

impl Recovery for Notification {
    fn scoped_action(
        self,
        action: impl Fn(
            &mut Notification,
            &mut Window,
            &mut Context<Notification>,
        ) -> component::button::Button
        + 'static,
    ) -> Feedback {
        Feedback {
            message: self.action(action),
            scoped: true,
        }
    }
}

#[derive(Default)]
struct Scope {
    active: HashSet<&'static str>,
    recoveries: HashSet<&'static str>,
}

impl Scope {
    fn reconcile(&mut self, keys: Vec<&'static str>) -> (Vec<&'static str>, Vec<&'static str>) {
        let current: HashSet<_> = keys.into_iter().collect();
        let added = current.difference(&self.active).copied().collect();
        let dismissed = self
            .active
            .difference(&current)
            .copied()
            .filter(|key| self.recoveries.remove(key))
            .collect();
        self.active = current;
        (added, dismissed)
    }

    fn record(&mut self, key: &'static str, recovery: bool) {
        if recovery {
            self.recoveries.insert(key);
        }
    }

    fn release(&mut self) -> Vec<&'static str> {
        self.recoveries.drain().collect()
    }
}

/// Retry actions share Kit styling while each owner retains request semantics.
pub(crate) fn retry(id: impl Into<ElementId>) -> component::button::Button {
    use component::Sizable as _;
    use component::button::ButtonVariants as _;
    component::button::Button::new(id)
        .ghost()
        .small()
        .icon(component::Icon::empty().path("icons/rotate-ccw.svg"))
        .tooltip(tr("chat_retry"))
        .accessibility_label(tr("chat_retry"))
}
/// Present operation feedback through Kit's transient toast layer.
pub(crate) fn toast(
    window: &mut Window,
    summary: SharedString,
    message: Notification,
    cx: &mut App,
) {
    deliver(window, message, cx);
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
}

struct Info;

pub(crate) fn error(title: &str, detail: &str, window: &mut Window, cx: &mut App) {
    let mut entry = sailry_node_runtime::logging::Entry::new(
        sailry_node_runtime::logging::Area::Desktop,
        sailry_node_runtime::logging::Phase::Failed,
    );
    entry.reason = Some("operation_feedback");
    entry.emit();
    deliver(
        window,
        diagnostic(detail.to_owned().into()).when(!title.trim().is_empty(), |note| {
            note.title(title.to_owned())
        }),
        cx,
    );
}

pub(crate) fn diagnostic(message: SharedString) -> Notification {
    Notification::new()
        .with_type(NotificationType::Error)
        .content(move |_, _, _| {
            div()
                .id("error-detail")
                .test_support()
                .debug_selector(|| "error-toast-detail".into())
                .role(Role::Label)
                .aria_label(message.clone())
                .max_h_24()
                .overflow_y_scroll()
                .text_sm()
                .on_scroll(|_, _, cx| cx.stop_propagation())
                .child(message.clone())
                .into_any_element()
        })
}

pub(crate) fn fault_message(error: &sailry_protocol::Fault) -> SharedString {
    if let Some(sailry_protocol::FaultDetails::VersionMismatch {
        update_host,
        unverified,
        ..
    }) = &error.details
    {
        tr(match (*update_host, *unverified) {
            (true, false) => "host_version_older",
            (false, false) => "host_version_newer",
            (true, true) => "host_version_unknown",
            (false, true) => "controller_version_unknown",
        })
    } else {
        error.message.clone().into()
    }
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

pub(crate) fn observe_with<T: 'static, Errors, Built>(
    window: &Window,
    cx: &mut Context<T>,
    errors: Errors,
    build: impl Fn(&T, &'static str, &mut Context<T>) -> Built + 'static,
) where
    Errors: Fn(&T, &App) -> Vec<&'static str> + 'static,
    Built: Into<Feedback>,
{
    let window = window.window_handle();
    let scope = Rc::new(RefCell::new(Scope::default()));
    let released = scope.clone();
    let owner = cx.entity_id();
    cx.on_release(move |_, cx| {
        let keys = released.borrow_mut().release();
        _ = window.update(cx, |_, window, cx| {
            for key in keys {
                window.remove_notification1::<Errors>((key, owner), cx);
            }
        });
    })
    .detach();
    cx.observe_self(move |view, cx| {
        let (added, dismissed) = scope.borrow_mut().reconcile(errors(view, cx));
        let owner = cx.entity_id();
        if !added.is_empty() || !dismissed.is_empty() {
            let messages: Vec<_> = added
                .into_iter()
                .map(|key| {
                    let mut diagnostic = sailry_node_runtime::logging::Entry::new(
                        sailry_node_runtime::logging::Area::Desktop,
                        sailry_node_runtime::logging::Phase::Failed,
                    );
                    diagnostic.reason = Some(key);
                    diagnostic.emit();
                    let built = build(view, key, cx).into();
                    scope.borrow_mut().record(key, built.scoped);
                    (tr(key), built.message.id1::<Errors>((key, owner)))
                })
                .collect();
            _ = window.update(cx, |_, window, cx| {
                for key in dismissed {
                    // Resolving a live condition must not retract the outcome
                    // before Kit's normal reading interval. Recovery actions,
                    // however, cannot outlive the error that authorizes them.
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

mod node;
pub(crate) use node::observe as observe_node;

#[cfg(test)]
mod tests;
