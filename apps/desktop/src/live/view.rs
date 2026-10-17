use super::*;
use crate::{preview::Page, sidebar::Row};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    collapsible::Collapsible,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;

impl Shell {
    pub(crate) fn select_live_host(
        &mut self,
        node: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.session_scope.open.is_empty()
            && self.guard_file_navigation(window, cx, move |shell, window, cx| {
                shell.select_live_host(node, window, cx)
            })
        {
            return;
        }
        self.park_session_panel(window, cx);
        if let Some(live) = &mut self.live {
            live.select(node, cx);
        }
        self.page = Page::Host;
        self.close_resource_panel(cx);
        cx.notify();
    }

    pub(super) fn select_live_project(
        &mut self,
        project: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.session_scope.open.is_empty()
            && self.guard_file_navigation(window, cx, move |shell, window, cx| {
                shell.select_live_project(project, window, cx)
            })
        {
            return;
        }
        self.park_session_panel(window, cx);
        if let Some(live) = &mut self.live {
            live.project = Some(project);
        }
        self.page = Page::Project;
        self.close_resource_panel(cx);
        cx.notify();
    }

    pub(crate) fn live_host_rows(&self, cx: &mut Context<Self>) -> AnyElement {
        let live = self.live.as_ref().unwrap();
        v_flex()
            .gap_0p5()
            .children(
                std::iter::once(live.services.local.target())
                    .chain(
                        live.hosts
                            .keys()
                            .copied()
                            .filter(|node| *node != live.services.local.target()),
                    )
                    .map(|node| {
                        let name = live.name(node);
                        let mismatch = live
                            .host_versions
                            .get(&node)
                            .and_then(|version| sailry_link::check_host(version.as_deref()).err());
                        let update_host = mismatch.as_ref().is_some_and(|error| {
                            matches!(
                                error.details,
                                Some(sailry_protocol::FaultDetails::VersionMismatch {
                                    update_host: true,
                                    ..
                                })
                            )
                        });
                        self.sidebar_row(
                            SharedString::from(format!("live-host-{}", node_key(node))),
                            Row::LiveHost(node),
                            live.selected == node,
                            cx,
                        )
                        .debug_selector(move || format!("live-host-{}", node_key(node)))
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |shell, event, window, cx| {
                                shell.live_resource_menu(
                                    node,
                                    menus::Target::Host,
                                    event,
                                    window,
                                    cx,
                                );
                            }),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(Icon::new(IconName::Cpu))
                                .child(div().flex_1().min_w_0().truncate().child(name))
                                .when(
                                    live.host_versions.get(&node).is_some_and(|version| {
                                        sailry_link::check_host(version.as_deref()).is_err()
                                    }),
                                    |row| {
                                        row.child(
                                            Button::new(SharedString::from(format!(
                                                "host-update-{}",
                                                node_key(node)
                                            )))
                                            .icon(IconName::ArrowUp)
                                            .ghost()
                                            .xsmall()
                                            .tooltip(tr(if update_host {
                                                "host_update"
                                            } else {
                                                "updates_check"
                                            }))
                                            .on_click(cx.listener(move |shell, _, window, cx| {
                                                cx.stop_propagation();
                                                shell.update_live_host(node, window, cx);
                                            })),
                                        )
                                    },
                                ),
                        )
                        .on_click(cx.listener(
                            move |shell, _, window, cx| shell.select_live_host(node, window, cx),
                        ))
                    }),
            )
            .into_any_element()
    }

    fn update_live_host(&mut self, node: NodeId, window: &mut Window, cx: &mut Context<Self>) {
        use crate::theme::DialogStyle as _;
        let Some(version) = self
            .live
            .as_ref()
            .and_then(|live| live.host_versions.get(&node))
        else {
            return;
        };
        let Err(error) = sailry_link::check_host(version.as_deref()) else {
            return;
        };
        if matches!(
            error.details,
            Some(sailry_protocol::FaultDetails::VersionMismatch {
                update_host: true,
                ..
            })
        ) {
            self.update_host(node, window, cx);
        } else if let Some(version) = version.clone() {
            let panel = cx.new(|cx| crate::updater::Panel::new(window, cx));
            window.open_dialog(cx, move |dialog, _, cx| {
                dialog
                    .form_title(tr("updates"), cx)
                    .w(px(560.))
                    .child(panel.clone())
            });
            crate::updater::check_version(&version, cx);
        }
    }

    pub(crate) fn live_project_rows(&self, cx: &mut Context<Self>) -> AnyElement {
        let live = self.live.as_ref().unwrap();
        let projects = live
            .view
            .snapshot
            .as_ref()
            .map(|view| view.projects.as_slice())
            .unwrap_or_default();
        v_flex()
            .gap_0p5()
            .children(projects.iter().map(|project| {
                let id = project.id;
                let node = live.selected;
                let open = self.sidebar.project_open(node, id);
                Collapsible::new()
                    .open(open)
                    .gap_0p5()
                    .child(
                        self.sidebar_row(
                            SharedString::from(format!("live-project-{id}")),
                            Row::LiveProject(id),
                            live.project == Some(id) && self.page == Page::Project,
                            cx,
                        )
                        .debug_selector(move || format!("live-project-{id}"))
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |shell, event, window, cx| {
                                shell.live_resource_menu(
                                    node,
                                    menus::Target::Project(id),
                                    event,
                                    window,
                                    cx,
                                );
                            }),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(crate::workspace::appearance::project(
                                    &project.appearance,
                                    cx,
                                ))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .child(project.name.clone()),
                                )
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "project-disclosure-{id}"
                                    )))
                                    .ghost()
                                    .xsmall()
                                    .debug_selector(move || format!("project-disclosure-{id}"))
                                    .when(
                                        self.sidebar.hovered != Some(Row::LiveProject(id)),
                                        |button| {
                                            button.opacity(0.).focus(|style| style.opacity(1.))
                                        },
                                    )
                                    .icon(if open {
                                        IconName::ChevronDown
                                    } else {
                                        IconName::ChevronRight
                                    })
                                    .accessibility_label(tr(if open {
                                        "workspace_collapse"
                                    } else {
                                        "workspace_expand"
                                    }))
                                    .on_click(cx.listener(move |shell, _, _, cx| {
                                        cx.stop_propagation();
                                        shell.sidebar.toggle_project(node, id);
                                        cx.notify();
                                    })),
                                ),
                        )
                        .on_click(cx.listener(move |shell, _, _, cx| {
                            shell.sidebar.toggle_project(node, id);
                            cx.notify();
                        })),
                    )
                    .content(self.live_resource_rows(Some(id), cx))
            }))
            .into_any_element()
    }
}
