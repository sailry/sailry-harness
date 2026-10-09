use super::*;
use crate::{
    settings::{
        entry::Entry,
        group::{Group, Row},
    },
    tr,
};
use gpui_kit::{
    component::{button::Button, separator::Separator, switch::Switch},
    prelude::FluentBuilder,
};
use sailry_protocol::Command;

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.workspace.is_none() {
            return v_flex()
                .gap_6()
                .when(self.loading, |view| {
                    view.child(gpui_kit::component::spinner::Spinner::new())
                })
                .child(
                    Button::new("code-refresh")
                        .debug_selector(|| "code-refresh".into())
                        .label(tr("code_refresh"))
                        .disabled(self.loading)
                        .on_click(cx.listener(|panel, _, window, cx| panel.load(window, cx))),
                )
                .into_any_element();
        }
        let locked = self.locked();
        let mut languages =
            Group::new("code_languages").empty(IconName::Cpu, "code_languages_empty");
        if let Some(workspace) = &self.workspace {
            for (language_index, language) in workspace.languages.iter().enumerate() {
                let mut rows = v_flex()
                    .w_full()
                    .min_w_0()
                    .gap_0()
                    .debug_selector(move || format!("code-language-{language_index}"))
                    .child(div().py_3().font_semibold().child(language.name.clone()));
                for (index, candidate) in language.servers.iter().enumerate() {
                    if index > 0 {
                        rows = rows.child(
                            div()
                                .relative()
                                .w_full()
                                .h(px(1.))
                                .flex_shrink_0()
                                .child(Separator::horizontal().size_full()),
                        );
                    }
                    let server = candidate.server.clone();
                    let toggle_server = server.clone();
                    let owner = cx.entity();
                    let edit_server = server.clone();
                    let disabled = self
                        .draft
                        .as_ref()
                        .and_then(|draft| draft.servers.get(&server))
                        .map_or(candidate.disabled, |value| value.disabled);
                    let running = self
                        .servers
                        .iter()
                        .find(|current| candidate.startup.as_ref() == Some(&current.startup));
                    let status = running.map_or(candidate.status, |current| current.status);
                    let status_key = status_key(status);
                    let mut entry = Entry::new(
                        format!("code-server-{language_index}-{index}"),
                        v_flex()
                            .gap_1()
                            .min_w_0()
                            .overflow_hidden()
                            .child(div().min_h_6().truncate().child(server.clone()))
                            .child(
                                div()
                                    .text_sm()
                                    .truncate()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(tr(status_key)),
                            )
                            .when_some(
                                running.and_then(|server| server.progress.as_ref()),
                                |view, progress| {
                                    view.child(div().text_sm().child(progress.clone()))
                                },
                            ),
                    )
                    .control_width(rems(3.))
                    .actions_width(rems(7.5))
                    .control(
                        div()
                            .debug_selector(move || {
                                format!("code-enabled-{language_index}-{index}")
                            })
                            .child(
                                Switch::new(SharedString::from(format!(
                                    "code-enabled-{language_index}-{index}"
                                )))
                                .checked(!disabled)
                                .accessibility_label(tr("code_enabled"))
                                .disabled(locked)
                                .on_click(cx.listener(
                                    move |panel, value: &bool, window, cx| {
                                        if let Some(draft) = panel.draft.as_mut() {
                                            draft
                                                .servers
                                                .entry(toggle_server.clone())
                                                .or_insert(code::Override {
                                                    disabled: false,
                                                    command: None,
                                                    args: None,
                                                    config: None,
                                                    environment: None,
                                                })
                                                .disabled = !*value;
                                        }
                                        panel.save(window, cx);
                                    },
                                )),
                            ),
                    )
                    .action(
                        Button::new(SharedString::from(format!(
                            "code-edit-{language_index}-{index}"
                        )))
                        .debug_selector(move || format!("code-edit-{language_index}-{index}"))
                        .icon(IconName::Settings2)
                        .tooltip(tr("code_custom_command"))
                        .accessibility_label(tr("code_custom_command"))
                        .disabled(locked)
                        .on_click(move |_, window, cx| {
                            editor::open(owner.clone(), edit_server.clone(), window, cx)
                        }),
                    );
                    if let Some(startup) = &candidate.startup {
                        let startup = startup.clone();
                        let approved = candidate.startup_approved;
                        let owner = cx.entity();
                        entry = entry.action(
                            Button::new(SharedString::from(format!(
                                "code-start-{language_index}-{index}"
                            )))
                            .debug_selector(move || format!("code-start-{language_index}-{index}"))
                            .icon(IconName::Play)
                            .tooltip(tr("code_start"))
                            .accessibility_label(tr("code_start"))
                            .disabled(
                                locked
                                    || disabled
                                    || matches!(
                                        status,
                                        code::Status::Starting
                                            | code::Status::Indexing
                                            | code::Status::Ready
                                    ),
                            )
                            .on_click(move |_, window, cx| {
                                consent::startup(
                                    owner.clone(),
                                    startup.clone(),
                                    approved,
                                    window,
                                    cx,
                                )
                            }),
                        );
                    }
                    if let Some(install) = candidate
                        .package
                        .as_ref()
                        .and_then(|package| package.install.as_ref())
                    {
                        let install = install.clone();
                        let owner = cx.entity();
                        entry = entry.action(
                            Button::new(SharedString::from(format!(
                                "code-install-{language_index}-{index}"
                            )))
                            .debug_selector(move || {
                                format!("code-install-{language_index}-{index}")
                            })
                            .icon(IconName::ArrowDown)
                            .tooltip(tr("code_install"))
                            .accessibility_label(tr("code_install"))
                            .disabled(
                                locked
                                    || self.installations.iter().any(|current| {
                                        current.package == install.package && busy(current.status)
                                    }),
                            )
                            .on_click(move |_, window, cx| {
                                consent::install(owner.clone(), install.clone(), window, cx)
                            }),
                        );
                    }
                    if let Some(reason) = candidate
                        .package
                        .as_ref()
                        .and_then(|package| package.unavailable.as_ref())
                    {
                        let reason = reason.clone();
                        entry = entry.action(
                            Button::new(SharedString::from(format!(
                                "code-package-details-{language_index}-{index}"
                            )))
                            .icon(IconName::Info)
                            .tooltip(tr("code_details"))
                            .accessibility_label(tr("code_details"))
                            .on_click(move |_, window, cx| {
                                window.push_notification(
                                    crate::feedback::diagnostic(reason.clone().into()),
                                    cx,
                                )
                            }),
                        );
                    }
                    rows = rows.child(entry);
                }
                languages = languages.child(rows);
            }
        }
        let mut installations =
            Group::new("code_installations").empty(IconName::Inbox, "code_installations_empty");
        for (index, installation) in self.installations.iter().enumerate() {
            let package = installation.package.clone();
            let mut entry = Entry::new(
                format!("code-installation-{index}"),
                v_flex()
                    .gap_1()
                    .child(div().child(package.clone()))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr(install_key(installation.status))),
                    )
                    .when_some(installation.progress.as_ref(), |view, progress| {
                        view.child(div().text_sm().child(progress.clone()))
                    }),
            );
            if busy(installation.status) {
                entry = entry.action(
                    Button::new(SharedString::from(format!("code-cancel-{index}")))
                        .debug_selector(move || format!("code-cancel-{index}"))
                        .label(tr("code_cancel_install"))
                        .disabled(self.cancelling.is_some())
                        .on_click(cx.listener(move |panel, _, window, cx| {
                            panel.cancel(package.clone(), window, cx)
                        })),
                );
            }
            if let Some(receipt) = &installation.receipt {
                let owner = cx.entity();
                let package = receipt.package.clone();
                let version = receipt.version.clone();
                entry = entry.action(
                    Button::new(SharedString::from(format!("code-uninstall-{index}")))
                        .debug_selector(move || format!("code-uninstall-{index}"))
                        .label(tr("code_uninstall"))
                        .disabled(self.pending || self.request.is_some())
                        .on_click(move |_, window, cx| {
                            consent::uninstall(
                                owner.clone(),
                                package.clone(),
                                version.clone(),
                                window,
                                cx,
                            )
                        }),
                );
            }
            if let Some(error) = &installation.error {
                let error = error.clone();
                entry = entry.action(
                    Button::new(SharedString::from(format!("code-install-details-{index}")))
                        .label(tr("code_details"))
                        .on_click(move |_, window, cx| {
                            window.push_notification(
                                crate::feedback::diagnostic(error.clone().into())
                                    .title(tr("code_failed")),
                                cx,
                            )
                        }),
                );
            }
            installations = installations.child(entry);
        }
        let controls = Group::new("code_settings")
            .child(Row::new(
                "code_after_edit",
                div().debug_selector(|| "code-after-edit".into()).child(
                    Switch::new("code-after-edit")
                        .checked(self.draft.as_ref().is_some_and(|draft| draft.after_edit))
                        .disabled(locked)
                        .accessibility_label(tr("code_after_edit"))
                        .on_click(cx.listener(|panel, value, window, cx| {
                            if let Some(draft) = panel.draft.as_mut() {
                                draft.after_edit = *value;
                            }
                            panel.save(window, cx);
                        })),
                ),
            ))
            .action(
                Button::new("code-refresh")
                    .label(tr("code_refresh"))
                    .disabled(self.pending || self.request.is_some())
                    .debug_selector(|| "code-refresh".into())
                    .loading(self.loading)
                    .on_click(cx.listener(|panel, _, window, cx| panel.load(window, cx))),
            );
        let rules = Group::new("code_rules")
            .child(
                Row::new(
                    "code_rules_revision",
                    div().text_sm().child(
                        self.rules
                            .as_ref()
                            .map(|rules| format!("{} · {}", rules.helix_commit, rules.mason_tag))
                            .unwrap_or_default(),
                    ),
                )
                .wide(),
            )
            .action(
                Button::new("code-update-rules")
                    .label(tr("code_update_rules"))
                    .disabled(locked)
                    .debug_selector(|| "code-update-rules".into())
                    .on_click(cx.listener(|panel, _, window, cx| {
                        panel.submit(Command::UpdateCodeRules, window, cx)
                    })),
            );
        v_flex()
            .gap_6()
            .debug_selector(|| "code-settings".into())
            .child(controls)
            .when(self.request.is_some() && !self.pending, |view| {
                view.child(
                    Button::new("code-retry")
                        .debug_selector(|| "code-retry".into())
                        .label(tr("code_retry"))
                        .on_click(cx.listener(|panel, _, window, cx| panel.retry(window, cx))),
                )
            })
            .when(self.error == Some("code_conflict"), |view| {
                view.child(
                    Button::new("code-reload")
                        .debug_selector(|| "code-reload".into())
                        .label(tr("code_discard_reload"))
                        .on_click(cx.listener(|panel, _, window, cx| {
                            panel.draft = panel.saved.clone();
                            panel.load(window, cx);
                        })),
                )
            })
            .when(
                self.workspace
                    .as_ref()
                    .is_some_and(|workspace| workspace.truncated),
                |view| view.child(div().text_sm().child(tr("code_languages_partial"))),
            )
            .child(languages)
            .child(installations)
            .child(rules)
            .into_any_element()
    }
}

fn status_key(status: code::Status) -> &'static str {
    match status {
        code::Status::Starting => "code_starting",
        code::Status::Indexing => "code_indexing",
        code::Status::Ready => "code_ready",
        code::Status::Failed => "code_status_failed",
        code::Status::Stopped => "code_stopped",
        code::Status::NotInstalled => "code_not_installed",
    }
}
fn install_key(status: code::InstallStatus) -> &'static str {
    match status {
        code::InstallStatus::Preparing => "code_preparing",
        code::InstallStatus::Downloading => "code_downloading",
        code::InstallStatus::Installing => "code_installing",
        code::InstallStatus::Ready => "code_ready",
        code::InstallStatus::Failed => "code_status_failed",
        code::InstallStatus::Cancelled => "code_cancelled",
    }
}
fn busy(status: code::InstallStatus) -> bool {
    matches!(
        status,
        code::InstallStatus::Preparing
            | code::InstallStatus::Downloading
            | code::InstallStatus::Installing
    )
}
