use super::*;
use gpui_kit::component::{
    group_box::{GroupBox, GroupBoxVariants},
    spinner::Spinner,
};

impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.refresh {
            self.refresh = false;
            self.check(window, cx);
        }
        let busy = self.checking || self.pending.is_some();
        v_flex()
            .w_full()
            .gap_2()
            .text_sm()
            .line_height(relative(1.25))
            .debug_selector(|| "permissions-modal".into())
            .children(
                self.cards
                    .iter()
                    .filter(|card| card.resource.is_system())
                    .map(|card| {
                        let resource = card.resource;
                        let status = card.status;
                        let pending = self.pending == Some(resource);
                        let unavailable = matches!(status, Status::Remote | Status::Unavailable);
                        let disabled = busy
                            || unavailable
                            || card.request.is_none()
                            || matches!(status, Status::Granted | Status::NotNeeded);
                        GroupBox::new()
                            .outline()
                            .content_style(StyleRefinement::default().p_3().gap_0())
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_3()
                                    .flex_wrap()
                                    .debug_selector(move || format!("{}-row", resource.key()))
                                    .child(
                                        v_flex()
                                            .gap_0p5()
                                            .flex_1()
                                            .min_w_32()
                                            .max_w_full()
                                            .debug_selector(move || {
                                                format!("{}-summary", resource.key())
                                            })
                                            .child(div().font_medium().child(tr(resource.key())))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .debug_selector(move || {
                                                        format!(
                                                            "{}-{}",
                                                            resource.key(),
                                                            status.key()
                                                        )
                                                    })
                                                    .child(tr(status.key())),
                                            ),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .flex_wrap()
                                            .justify_end()
                                            .flex_shrink_0()
                                            .max_w_full()
                                            .debug_selector(move || {
                                                format!("{}-actions", resource.key())
                                            })
                                            .when(card.check.is_some(), |row| {
                                                row.child(
                                                    Button::new(format!(
                                                        "{}-check",
                                                        resource.key()
                                                    ))
                                                    .ghost()
                                                    .small()
                                                    .label(tr("permission_check"))
                                                    .disabled(busy || unavailable)
                                                    .debug_selector(move || {
                                                        format!("{}-check", resource.key())
                                                    })
                                                    .on_click(cx.listener(
                                                        |panel, _, window, cx| {
                                                            panel.check(window, cx)
                                                        },
                                                    )),
                                                )
                                            })
                                            .when(
                                                card.settings.is_some()
                                                    && resource != Resource::FullDisk
                                                    && !matches!(
                                                        status,
                                                        Status::Denied | Status::Restricted
                                                    ),
                                                |row| {
                                                    let url = card.settings.unwrap();
                                                    row.child(
                                                        Button::new(format!(
                                                            "{}-settings",
                                                            resource.key()
                                                        ))
                                                        .ghost()
                                                        .small()
                                                        .label(tr("permission_settings"))
                                                        .disabled(busy || unavailable)
                                                        .debug_selector(move || {
                                                            format!("{}-settings", resource.key())
                                                        })
                                                        .on_click(move |_, _, cx| cx.open_url(url)),
                                                    )
                                                },
                                            )
                                            .child(
                                                Button::new(resource.key())
                                                    .small()
                                                    .label(tr(
                                                        if resource == Resource::FullDisk
                                                            || matches!(
                                                                status,
                                                                Status::Denied | Status::Restricted
                                                            ) && card.settings.is_some()
                                                        {
                                                            "permission_settings"
                                                        } else {
                                                            "permission_request"
                                                        },
                                                    ))
                                                    .when(pending, |button| {
                                                        button.icon(Spinner::new())
                                                    })
                                                    .loading(pending)
                                                    .disabled(if resource == Resource::FullDisk {
                                                        busy || unavailable
                                                            || card.settings.is_none()
                                                    } else {
                                                        if matches!(
                                                            status,
                                                            Status::Denied | Status::Restricted
                                                        ) {
                                                            busy || unavailable
                                                                || card.settings.is_none()
                                                        } else {
                                                            disabled
                                                        }
                                                    })
                                                    .debug_selector(move || {
                                                        format!("{}-request", resource.key())
                                                    })
                                                    .on_click(cx.listener(
                                                        move |panel, _, window, cx| {
                                                            panel.request(resource, window, cx)
                                                        },
                                                    )),
                                            ),
                                    ),
                            )
                    }),
            )
            .when(
                self.cards.iter().any(|card| {
                    card.resource == Resource::FullDisk
                        && !matches!(
                            card.status,
                            Status::Granted | Status::Remote | Status::Unavailable
                        )
                }),
                |column| {
                    column.child(
                        v_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(tr("permission_disk_help")),
                            )
                            .child(
                                Button::new("permissions-show-app")
                                    .ghost()
                                    .small()
                                    .label(tr("permission_show_app"))
                                    .disabled(busy)
                                    .debug_selector(|| "permissions-show-app".into())
                                    .on_click(|_, window, cx| {
                                        if let Some(path) = app::bundle() {
                                            cx.reveal_path(&path);
                                        } else {
                                            crate::feedback::info(
                                                "",
                                                tr("permission_app_unavailable").as_ref(),
                                                window,
                                                cx,
                                            );
                                        }
                                    }),
                            ),
                    )
                },
            )
    }
}
