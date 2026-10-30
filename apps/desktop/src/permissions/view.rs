use super::*;
use crate::settings::group::{Group, Row};

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
            .child(
                Group::new("permission_title")
                    .action(
                        Button::new("permissions-check")
                            .ghost()
                            .small()
                            .icon(IconName::RefreshCw)
                            .tooltip(tr("permission_check"))
                            .accessibility_label(tr("permission_check"))
                            .disabled(
                                busy || !self.cards.iter().any(|card| {
                                    card.resource.is_system()
                                        && card.check.is_some()
                                        && !matches!(
                                            card.status,
                                            Status::Remote | Status::Unavailable
                                        )
                                }),
                            )
                            .debug_selector(|| "permissions-check".into())
                            .on_click(cx.listener(|panel, _, window, cx| panel.check(window, cx))),
                    )
                    .children(self.rows().into_iter().map(|card| {
                        let resource = card.resource;
                        let status = card.status;
                        let granted = matches!(status, Status::Granted | Status::NotNeeded);
                        let opens_settings = self.cards.iter().any(|member| {
                            member.resource == resource
                                && member.actionable()
                                && member.settings_action().is_some()
                        });
                        let action = if opens_settings {
                            "permission_settings"
                        } else {
                            "permission_request"
                        };
                        let pending = self.pending == Some(resource);
                        let unavailable = matches!(status, Status::Remote | Status::Unavailable);
                        let disabled = busy
                            || !self
                                .cards
                                .iter()
                                .any(|member| member.resource == resource && member.actionable());
                        Row::new(
                            resource.key(),
                            h_flex()
                                .gap_2()
                                .flex_wrap()
                                .justify_end()
                                .debug_selector(move || format!("{}-actions", resource.key()))
                                .when(
                                    card.settings.is_some()
                                        && (resource != Resource::FullDisk || granted)
                                        && !matches!(status, Status::Denied | Status::Restricted),
                                    |row| {
                                        let url = card.settings.unwrap();
                                        row.child(
                                            Button::new(format!("{}-settings", resource.key()))
                                                .ghost()
                                                .small()
                                                .icon(IconName::Settings2)
                                                .tooltip(tr("permission_settings"))
                                                .accessibility_label(tr("permission_settings"))
                                                .disabled(busy || unavailable)
                                                .debug_selector(move || {
                                                    format!("{}-settings", resource.key())
                                                })
                                                .on_click(cx.listener(
                                                    move |panel, _, window, cx| {
                                                        if resource == Resource::FullDisk {
                                                            panel.disk_settings(window, cx);
                                                        } else {
                                                            cx.open_url(url);
                                                        }
                                                    },
                                                )),
                                        )
                                    },
                                )
                                .when(!granted, |row| {
                                    row.child(
                                        Button::new(resource.key())
                                            .small()
                                            .label(tr(action))
                                            .loading(pending)
                                            .disabled(disabled)
                                            .debug_selector(move || {
                                                format!("{}-request", resource.key())
                                            })
                                            .on_click(cx.listener(move |panel, _, window, cx| {
                                                panel.request(resource, window, cx)
                                            })),
                                    )
                                }),
                        )
                        .description(status.key())
                    })),
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
                    // A quiet footnote under the list; the action stays a regular button.
                    column.child(
                        h_flex()
                            .w_full()
                            .px_1()
                            .gap_2()
                            .items_center()
                            .text_color(cx.theme().muted_foreground)
                            .debug_selector(|| "permissions-disk-help".into())
                            .child(Icon::new(IconName::Info).small().flex_shrink_0())
                            .child(div().flex_1().min_w_0().child(tr("permission_disk_help")))
                            .child(
                                Button::new("permissions-show-app")
                                    .ghost()
                                    .small()
                                    .icon(IconName::FolderOpen)
                                    .label(tr("permission_show_app"))
                                    .flex_shrink_0()
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
