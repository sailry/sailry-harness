use super::service::{Notice, Service, State};
use crate::{theme::DialogStyle as _, tr};
use gpui_kit::{
    component::{
        WindowExt,
        button::{Button, ButtonVariants},
        dialog::DialogButtonProps,
        notification::Notification,
    },
    *,
};

pub(super) fn show(service: Entity<Service>, notice: &Notice, cx: &mut App) {
    let Some(handle) = cx.active_window().or_else(|| cx.windows().first().copied()) else {
        return;
    };
    let summary: SharedString = notice.version.as_ref().map_or_else(
        || tr(notice.key),
        |version| {
            rust_i18n::t!("updates_ready", version = version)
                .to_string()
                .into()
        },
    );
    let message = if notice.error {
        Notification::error(summary.clone())
    } else {
        Notification::success(summary.clone())
    };
    let message = if let Some(version) = notice.version.clone() {
        message.action(move |_, _, cx| {
            let service = service.clone();
            let version = version.clone();
            Button::new("updates-ready-install")
                .primary()
                .label(tr("updates_install"))
                .on_click(cx.listener(move |toast, _, window, cx| {
                    if service.read(cx).state == State::Ready(version.clone()) {
                        confirm(service.clone(), window, cx);
                    }
                    toast.dismiss(window, cx);
                }))
        })
    } else if notice.error
        && let Some(failure) = service
            .read(cx)
            .failure
            .clone()
            .filter(|failure| failure.key == notice.key)
    {
        message.action(move |_, _, cx| {
            let failure = failure.clone();
            Button::new("updates-failure-details")
                .ghost()
                .label(tr("updates_details"))
                .on_click(cx.listener(move |toast, _, window, cx| {
                    crate::ui::details::open(
                        tr("updates_details").to_string(),
                        String::new(),
                        failure.detail.clone(),
                        window,
                        cx,
                    );
                    toast.dismiss(window, cx);
                }))
        })
    } else {
        message
    };
    let _ = handle.update(cx, |_, window, cx| {
        crate::feedback::toast(window, summary, message, cx);
    });
}

pub(super) fn confirm(service: Entity<Service>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let service = service.clone();
        dialog
            .form_title(tr("updates_install"), cx)
            .child(tr("updates_restart_description"))
            .button_props(
                DialogButtonProps::default()
                    .ok_text(tr("updates_install"))
                    .cancel_text(tr("settings_cancel")),
            )
            .on_ok(move |_, window, cx| {
                let service = service.clone();
                window.defer(cx, move |window, cx| {
                    service.update(cx, |service, cx| service.install(window, cx))
                });
                true
            })
    });
}
