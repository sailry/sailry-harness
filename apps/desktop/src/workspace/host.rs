use crate::{shell::Shell, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    description_list::DescriptionList,
    list::ListItem,
    scroll::ScrollableElement,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

mod activity;
pub(super) mod metrics;
mod runtime;
mod tunnels;
mod update;

impl Shell {
    pub(crate) fn observe_host_errors(window: &Window, cx: &mut Context<Self>) {
        crate::feedback::observe_with(
            window,
            cx,
            |shell: &Self, _| {
                [
                    shell
                        .workspace
                        .runtimes
                        .get(&shell.host)
                        .and_then(|state| state.error.as_ref())
                        .map(|_| "host_runtime_error"),
                    shell
                        .workspace
                        .tunnels
                        .get(&shell.host)
                        .and_then(|state| state.error),
                ]
                .into_iter()
                .flatten()
                .collect()
            },
            |shell, key, cx| {
                use gpui_kit::component::notification::Notification;
                let detail = match key {
                    "host_runtime_error" => shell
                        .workspace
                        .runtimes
                        .get(&shell.host)
                        .and_then(|state| state.error.clone()),
                    _ => None,
                };
                let notice = detail
                    .map(crate::feedback::diagnostic)
                    .unwrap_or_else(|| Notification::error(tr(key)));
                if shell
                    .workspace
                    .tunnels
                    .get(&shell.host)
                    .and_then(|state| state.error)
                    != Some(key)
                {
                    return notice;
                }
                let host = shell.host;
                let owner = cx.weak_entity();
                notice.action(move |_, _, cx| {
                    let owner = owner.clone();
                    Button::new("tunnels-retry")
                        .label(tr("tunnels_retry"))
                        .debug_selector(|| "tunnels-retry".into())
                        .on_click(cx.listener(move |toast, _, window, cx| {
                            toast.dismiss(window, cx);
                            _ = owner.update(cx, |shell, cx| {
                                if let Some(state) = shell.workspace.tunnels.get_mut(&host) {
                                    state.error = None;
                                    state.loaded = true;
                                }
                                cx.notify();
                            });
                        }))
                })
            },
        );
    }

    pub(crate) fn host_overview(&self, cx: &mut Context<Self>) -> AnyElement {
        let host = self.host;
        let active = self.host_activity(host);
        let projects = self
            .workspace
            .projects
            .values()
            .filter(|p| p.host == host)
            .count();
        let worktrees = self
            .workspace
            .worktrees
            .values()
            .filter(|w| self.workspace.projects[&w.project].host == host)
            .count();
        let sessions = self
            .workspace
            .sessions
            .iter()
            .filter(|(key, _)| key.0 == host)
            .count();
        let terminals = self
            .workspace
            .terminals
            .iter()
            .filter(|(key, _)| key.0 == host)
            .count();
        v_flex()
            .id("host-overview")
            .debug_selector(|| "host-overview".into())
            .size_full()
            .overflow_y_scrollbar()
            .items_center()
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(crate::preview::CONTENT_WIDTH))
                    .p_6()
                    .flex_shrink_0()
                    .gap_6()
                    .child(
                        h_flex()
                            .gap_4()
                            .py_4()
                            .child(Icon::new(IconName::Cpu).size_10())
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_1()
                                    .child(
                                        div()
                                            .debug_selector(|| "host-overview-name".into())
                                            .text_xl()
                                            .font_semibold()
                                            .truncate()
                                            .child(tr(if host == 0 {
                                                "local_host_name"
                                            } else {
                                                "remote_host_name"
                                            })),
                                    )
                                    .child(
                                        div()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(tr("workspace_host_status")),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .debug_selector(|| "host-platform".into())
                                            .child(tr(if host == 0 {
                                                "local_host_detail"
                                            } else {
                                                "remote_host_detail"
                                            })),
                                    ),
                            )
                            .when(host != 0, |row| {
                                row.child(
                                    Button::new("host-update")
                                        .small()
                                        .icon(IconName::ArrowUp)
                                        .label(tr("host_update"))
                                        .debug_selector(|| "host-update".into())
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.host_update(host, window, cx)
                                        })),
                                )
                            }),
                    )
                    .child(
                        DescriptionList::new()
                            .columns(1)
                            .bordered(false)
                            .item(
                                tr("workspace_host_type"),
                                tr(if host == 0 {
                                    "composer_host_local"
                                } else {
                                    "composer_host_remote"
                                }),
                                1,
                            )
                            .item(tr("projects"), projects.to_string(), 1)
                            .item(tr("workspace_worktrees"), worktrees.to_string(), 1)
                            .item(tr("workspace_sessions"), sessions.to_string(), 1)
                            .item(tr("terminal"), terminals.to_string(), 1)
                            .item(tr("host_active_preview"), active.len().to_string(), 1),
                    )
                    .child(
                        v_flex()
                            .gap_2()
                            .child(div().font_semibold().child(tr("projects")))
                            .when(projects == 0, |body| {
                                body.child(
                                    crate::empty_state::card_content(
                                        crate::empty_state::list(
                                            IconName::Folder,
                                            "project_empty",
                                            cx,
                                        )
                                        .child(
                                            Button::new("host-add-project")
                                                .debug_selector(|| "host-add-project".into())
                                                .primary()
                                                .icon(IconName::Plus)
                                                .label(tr("project_add"))
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.project_editor(host, None, window, cx);
                                                    },
                                                )),
                                        ),
                                        "project_empty".into(),
                                        cx,
                                    )
                                    .debug_selector(|| "host-no-projects".into()),
                                )
                            })
                            .children(
                                self.workspace
                                    .projects
                                    .iter()
                                    .filter(|(_, p)| p.host == host)
                                    .map(|(&id, project)| {
                                        ListItem::new(("host-project", id))
                                            .w_full()
                                            .debug_selector(move || format!("host-project-{id}"))
                                            .child(
                                                h_flex()
                                                    .gap_2()
                                                    .child(Icon::new(IconName::Folder))
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .min_w_0()
                                                            .truncate()
                                                            .child(project.name.clone()),
                                                    ),
                                            )
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.select_project(id, window, cx)
                                            }))
                                    }),
                            ),
                    )
                    .child(self.host_runtime(cx))
                    .child(self.host_activity_section(active, cx))
                    .child(self.host_tunnels(host, cx)),
            )
            .into_any_element()
    }
}
