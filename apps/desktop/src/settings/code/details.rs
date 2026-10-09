use super::*;
use crate::{
    settings::{entry::Entry, group::Group},
    theme::DialogStyle as _,
    tr,
};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    dialog::{DialogClose, DialogFooter},
};
use gpui_kit::prelude::FluentBuilder as _;

struct Details {
    owner: Entity<Panel>,
    row: rows::ServerRow,
}

pub(super) fn open(owner: Entity<Panel>, row: rows::ServerRow, window: &mut Window, cx: &mut App) {
    if !owner.read(cx).active {
        return;
    }
    let details = cx.new(|cx| {
        cx.observe(&owner, |_, _, cx| cx.notify()).detach();
        Details { owner, row }
    });
    window.open_dialog(cx, move |dialog, window, cx| {
        let state = details.read(cx);
        let panel = state.owner.read(cx);
        let row = state.current(cx);
        let locked = !panel.active || panel.locked();
        let install = row
            .candidate
            .as_ref()
            .and_then(|candidate| candidate.package.as_ref())
            .and_then(|package| package.install.clone());
        let receipt = row
            .installation
            .as_ref()
            .and_then(|installation| installation.receipt.clone());
        let mut footer = DialogFooter::new()
            .gap_2()
            .child(DialogClose::new().trigger(|button| {
                button
                    .label(tr("settings_cancel"))
                    .debug_selector(|| "code-details-close".into())
            }));
        if let Some(installation) = row.installation.as_ref().filter(|installation| {
            matches!(
                installation.status,
                code::InstallStatus::Preparing
                    | code::InstallStatus::Downloading
                    | code::InstallStatus::Installing
            )
        }) {
            footer = footer.child(
                Button::new("code-details-update")
                    .debug_selector(|| "code-details-update".into())
                    .label(tr(if row.installed() {
                        "code_update"
                    } else {
                        "code_install"
                    }))
                    .loading(true)
                    .disabled(true),
            );
            let owner = state.owner.clone();
            let package = installation.package.clone();
            footer = footer.child(
                Button::new("code-details-cancel-install")
                    .debug_selector(|| "code-details-cancel-install".into())
                    .label(tr("code_cancel_install"))
                    .disabled(!panel.active || panel.cancelling.is_some())
                    .on_click(move |_, window, cx| {
                        owner.update(cx, |panel, cx| panel.cancel(package.clone(), window, cx))
                    }),
            );
        } else if row.installed() {
            let owner = state.owner.clone();
            let update = install.clone();
            let outdated = receipt
                .as_ref()
                .zip(install.as_ref())
                .is_some_and(|(receipt, install)| receipt.version != install.version);
            footer = footer.child(
                Button::new("code-details-update")
                    .debug_selector(|| "code-details-update".into())
                    .label(tr("code_update"))
                    .disabled(locked || !outdated)
                    .on_click(move |_, window, cx| {
                        if owner.read(cx).active
                            && !owner.read(cx).locked()
                            && let Some(install) = &update
                        {
                            window.close_dialog(cx);
                            consent::install(owner.clone(), install.clone(), window, cx);
                        }
                    }),
            );
            if let Some(receipt) = receipt {
                let owner = state.owner.clone();
                footer = footer.child(
                    Button::new("code-details-uninstall")
                        .debug_selector(|| "code-details-uninstall".into())
                        .label(tr("code_uninstall"))
                        .disabled(locked)
                        .on_click(move |_, window, cx| {
                            if owner.read(cx).active && !owner.read(cx).locked() {
                                window.close_dialog(cx);
                                consent::uninstall(
                                    owner.clone(),
                                    receipt.package.clone(),
                                    receipt.version.clone(),
                                    window,
                                    cx,
                                );
                            }
                        }),
                );
            }
        } else {
            let owner = state.owner.clone();
            footer = footer.child(
                Button::new("code-details-install")
                    .primary()
                    .debug_selector(|| "code-details-install".into())
                    .label(tr("code_install"))
                    .disabled(locked || install.is_none())
                    .on_click(move |_, window, cx| {
                        if owner.read(cx).active
                            && !owner.read(cx).locked()
                            && let Some(install) = &install
                        {
                            window.close_dialog(cx);
                            consent::install(owner.clone(), install.clone(), window, cx);
                        }
                    }),
            );
        }
        dialog
            .form_title(div().min_w_0().truncate().child(row.server))
            .w((window.viewport_size().width - px(48.)).min(px(520.)))
            .child(details.clone())
            .footer(footer)
    });
}

impl Details {
    fn current(&self, cx: &App) -> rows::ServerRow {
        self.owner
            .read(cx)
            .rows()
            .into_iter()
            .find(|row| row.server == self.row.server)
            .unwrap_or_else(|| self.row.clone())
    }
}

impl Render for Details {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let row = self.current(cx);
        let mut parameters = Group::new("code_information")
            .heading(false)
            .child(Entry::new("code-detail-server", tr("code_server")).control(row.server.clone()));
        if let Some(candidate) = &row.candidate {
            parameters = parameters.child(
                Entry::new("code-detail-command", tr("code_command"))
                    .control(div().whitespace_normal().child(candidate.command.clone())),
            );
        }
        if let Some(package) = row
            .candidate
            .as_ref()
            .and_then(|candidate| candidate.package.as_ref())
        {
            parameters = parameters
                .child(
                    Entry::new("code-detail-package", tr("code_package"))
                        .control(package.name.clone()),
                )
                .child(
                    Entry::new("code-detail-version", tr("code_version")).control(
                        row.installation
                            .as_ref()
                            .and_then(|installation| installation.receipt.as_ref())
                            .map(|receipt| receipt.version.clone())
                            .unwrap_or_else(|| package.version.clone()),
                    ),
                )
                .child(
                    Entry::new("code-detail-licenses", tr("code_licenses"))
                        .control(package.licenses.join(", ")),
                );
            if let Some(homepage) = &package.homepage {
                parameters = parameters.child(
                    Entry::new("code-detail-source", tr("code_source"))
                        .control(div().whitespace_normal().child(homepage.clone())),
                );
            }
            if let Some(reason) = &package.unavailable {
                let reason = reason.clone();
                parameters = parameters.child(
                    Entry::new("code-detail-availability", tr("code_availability")).action(
                        Button::new("code-detail-availability-info")
                            .icon(IconName::Info)
                            .tooltip(tr("code_details"))
                            .accessibility_label(tr("code_details"))
                            .on_click(move |_, window, cx| {
                                window.push_notification(
                                    crate::feedback::diagnostic(reason.clone().into()),
                                    cx,
                                )
                            }),
                    ),
                );
            }
        } else if let Some(receipt) = row
            .installation
            .as_ref()
            .and_then(|installation| installation.receipt.as_ref())
        {
            parameters = parameters
                .child(
                    Entry::new("code-detail-package", tr("code_package"))
                        .control(receipt.package.clone()),
                )
                .child(
                    Entry::new("code-detail-version", tr("code_version"))
                        .control(receipt.version.clone()),
                )
                .child(
                    Entry::new("code-detail-source", tr("code_source"))
                        .control(div().whitespace_normal().child(receipt.purl.clone())),
                );
        }
        v_flex()
            .debug_selector(|| "code-server-details".into())
            .gap_4()
            .flex_none()
            .p_1()
            .when(!row.languages.is_empty(), |view| {
                view.child(
                    div()
                        .debug_selector(|| "code-server-description".into())
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .whitespace_normal()
                        .child(
                            rust_i18n::t!(
                                "code_server_description",
                                languages = row.languages.join(", ")
                            )
                            .to_string(),
                        ),
                )
            })
            .child(parameters)
    }
}
