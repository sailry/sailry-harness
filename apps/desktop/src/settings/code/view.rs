use super::*;
use crate::{
    settings::{
        entry::Entry,
        group::{Group, Row},
    },
    tr,
};
use gpui_kit::{
    component::{
        button::Button,
        switch::Switch,
        tab::{Tab, TabBar},
    },
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
                .when(!self.loading, |view| {
                    view.child(
                        Button::new("code-reload")
                            .debug_selector(|| "code-reload".into())
                            .label(tr("code_retry"))
                            .on_click(cx.listener(|panel, _, window, cx| panel.load(window, cx))),
                    )
                })
                .into_any_element();
        }
        let tabs = TabBar::new("code-server-tabs")
            .selected_index(self.tab)
            .child(
                Tab::new()
                    .debug_selector(|| "code-tab-installed".into())
                    .label(tr("code_installed")),
            )
            .child(
                Tab::new()
                    .debug_selector(|| "code-tab-available".into())
                    .label(tr("code_not_installed")),
            )
            .on_click(cx.listener(|panel, tab, _, cx| {
                panel.tab = *tab;
                cx.notify();
            }));
        let mut servers = Group::new("code_languages").heading(false).empty(
            IconName::Cpu,
            if self.tab == 0 {
                "code_installations_empty"
            } else {
                "code_languages_empty"
            },
        );
        for row in self
            .rows()
            .into_iter()
            .filter(|row| row.installed() == (self.tab == 0))
        {
            servers = servers.child(self.row(row, cx));
        }
        v_flex()
            .gap_6()
            .debug_selector(|| "code-settings".into())
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
            .child(v_flex().gap_4().child(tabs).child(servers))
            .into_any_element()
    }
}

impl Panel {
    pub(super) fn controls(&self, enabled: Row, cx: &mut Context<Self>) -> Group {
        Group::new("code_settings")
            .action(
                Button::new("code-update-rules")
                    .label(tr("code_update_rules"))
                    .disabled(self.locked())
                    .debug_selector(|| "code-update-rules".into())
                    .on_click(cx.listener(|panel, _, window, cx| {
                        panel.submit(Command::UpdateCodeRules, window, cx)
                    })),
            )
            .child(enabled)
            .child(
                Row::new(
                    "code_rules_revision",
                    div().text_sm().whitespace_normal().child(
                        self.rules
                            .as_ref()
                            .map(|rules| format!("{} · {}", rules.helix_commit, rules.mason_tag))
                            .unwrap_or_default(),
                    ),
                )
                .wide(),
            )
    }

    fn row(&self, row: rows::ServerRow, cx: &mut Context<Self>) -> Entry {
        let locked = self.locked();
        let server = row.server.clone();
        let running = row.candidate.as_ref().and_then(|candidate| {
            self.servers
                .iter()
                .find(|current| candidate.startup.as_ref() == Some(&current.startup))
        });
        let status = running.map_or_else(
            || row.candidate.as_ref().map(|candidate| candidate.status),
            |current| Some(current.status),
        );
        let status = row
            .installation
            .as_ref()
            .filter(|installation| busy(installation.status))
            .map(|installation| install_key(installation.status))
            .unwrap_or_else(|| status.map(status_key).unwrap_or("code_stopped"));
        let owner = cx.entity();
        let details_row = row.clone();
        let details_id = format!("code-details-{server}");
        let mut entry = Entry::new(
            format!("code-server-{server}"),
            v_flex()
                .gap_1()
                .min_w_0()
                .overflow_hidden()
                .child(div().min_h_6().truncate().child(server.clone()))
                .child(
                    h_flex()
                        .min_w_0()
                        .gap_3()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(div().truncate().child(row.languages.join(", ")))
                        .child(div().flex_shrink_0().child(tr(status))),
                )
                .when_some(
                    row.installation
                        .as_ref()
                        .and_then(|installation| installation.progress.as_ref()),
                    |view, progress| view.child(div().text_sm().child(progress.clone())),
                ),
        )
        .actions_width(rems(7.5))
        .action(
            Button::new(SharedString::from(details_id.clone()))
                .debug_selector(move || details_id.clone())
                .icon(IconName::Info)
                .tooltip(tr("code_details"))
                .accessibility_label(tr("code_details"))
                .on_click(move |_, window, cx| {
                    details::open(owner.clone(), details_row.clone(), window, cx)
                }),
        );
        if row.installed() {
            if let Some(candidate) = &row.candidate {
                let toggle_server = server.clone();
                let toggle_id = format!("code-enabled-{server}");
                let disabled = self
                    .draft
                    .as_ref()
                    .and_then(|draft| draft.servers.get(&server))
                    .map_or(candidate.disabled, |value| value.disabled);
                let owner = cx.entity();
                let edit_server = server.clone();
                let edit_id = format!("code-edit-{server}");
                entry = entry
                    .control_width(rems(3.))
                    .control(
                        div()
                            .debug_selector({
                                let id = toggle_id.clone();
                                move || id.clone()
                            })
                            .child(
                                Switch::new(SharedString::from(toggle_id))
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
                        Button::new(SharedString::from(edit_id.clone()))
                            .debug_selector(move || edit_id.clone())
                            .icon(IconName::Settings2)
                            .tooltip(tr("code_configure"))
                            .accessibility_label(tr("code_configure"))
                            .disabled(locked)
                            .on_click(move |_, window, cx| {
                                editor::open(owner.clone(), edit_server.clone(), window, cx)
                            }),
                    );
                if let Some(startup) = &candidate.startup {
                    let startup = startup.clone();
                    let approved = candidate.startup_approved;
                    let owner = cx.entity();
                    let start_id = format!("code-start-{server}");
                    entry = entry.action(
                        Button::new(SharedString::from(start_id.clone()))
                            .debug_selector(move || start_id.clone())
                            .icon(IconName::Play)
                            .tooltip(tr("code_start"))
                            .accessibility_label(tr("code_start"))
                            .disabled(
                                locked
                                    || !self.draft.as_ref().is_some_and(|draft| draft.enabled)
                                    || disabled
                                    || running.is_some_and(|current| {
                                        matches!(
                                            current.status,
                                            code::Status::Starting
                                                | code::Status::Indexing
                                                | code::Status::Ready
                                        )
                                    }),
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
            }
        } else if let Some(installation) = row
            .installation
            .as_ref()
            .filter(|installation| busy(installation.status))
        {
            let install_id = format!("code-install-{server}");
            entry = entry.action(
                Button::new(SharedString::from(install_id.clone()))
                    .debug_selector(move || install_id.clone())
                    .icon(IconName::ArrowDown)
                    .tooltip(tr("code_install"))
                    .accessibility_label(tr("code_install"))
                    .loading(true)
                    .disabled(true),
            );
            let cancel_id = format!("code-cancel-{server}");
            let package = installation.package.clone();
            entry = entry.action(
                Button::new(SharedString::from(cancel_id.clone()))
                    .debug_selector(move || cancel_id.clone())
                    .icon(IconName::CircleX)
                    .tooltip(tr("code_cancel_install"))
                    .accessibility_label(tr("code_cancel_install"))
                    .disabled(self.cancelling.is_some())
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.cancel(package.clone(), window, cx)
                    })),
            );
        } else {
            let install = row
                .candidate
                .as_ref()
                .and_then(|candidate| candidate.package.as_ref())
                .and_then(|package| package.install.clone());
            let install_id = format!("code-install-{server}");
            let installing = install.as_ref().is_some_and(|install| {
                self.pending && self.request.as_ref().is_some_and(|request| {
                    matches!(&request.command, Command::InstallCodeServer { install: pending } if pending.package == install.package)
                })
            });
            let owner = cx.entity();
            entry = entry.action(
                Button::new(SharedString::from(install_id.clone()))
                    .debug_selector(move || install_id.clone())
                    .icon(IconName::ArrowDown)
                    .tooltip(tr("code_install"))
                    .accessibility_label(tr("code_install"))
                    .loading(installing)
                    .disabled(locked || install.is_none())
                    .on_click(move |_, window, cx| {
                        if let Some(install) = &install {
                            consent::install(owner.clone(), install.clone(), window, cx);
                        }
                    }),
            );
        }
        entry
    }
}

pub(super) fn status_key(status: code::Status) -> &'static str {
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
