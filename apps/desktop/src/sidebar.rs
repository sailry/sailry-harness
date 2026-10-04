mod active;
mod groups;
mod order;
mod projects;
mod recent;
mod sessions;
mod settings;
#[cfg(test)]
mod tests;
mod title;
pub(crate) mod tree;
mod unassigned;

use crate::{preview::*, settings::Section, shell::Shell, theme, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    collapsible::Collapsible,
    list::ListItem,
    progress::Progress,
    scroll::ScrollableElement,
    separator::Separator,
    tooltip::Tooltip,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::BTreeSet;

pub(crate) const WIDTH_RANGE: std::ops::Range<f32> = 200. ..320.;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Row {
    Hosts,
    Projects,
    Recent,
    Active,
    Unassigned,
    ActiveSession(sailry_protocol::SessionId),
    RecentSession(sailry_protocol::SessionId),
    RecentPreview(usize, usize),
    LiveHost(sailry_protocol::NodeId),
    LiveProject(sailry_protocol::ProjectId),
    LiveTerminal(sailry_protocol::TerminalId),
    LiveSession(sailry_protocol::SessionId),
    Group(crate::panes::Target),
    Setting(Section),
    PluginSetting(usize),
    Host(usize),
    Project(usize),
    Session(usize, usize),
    Terminal(usize, usize),
}

#[derive(Default)]
pub struct State {
    order_drop: Option<(Row, bool)>,
    pub(crate) order_pending: bool,
    closed_groups: BTreeSet<crate::panes::Target>,
    hosts_closed: bool,
    projects_closed: bool,
    recent_open: bool,
    active_closed: bool,
    unassigned_closed: BTreeSet<sailry_protocol::NodeId>,
    active_scroll: ScrollHandle,
    // Explicit Kit viewports keep natural heights; the convenience wrapper fills its parent.
    projects_scroll: ScrollHandle,
    unassigned_scroll: ScrollHandle,
    recent_scroll: ScrollHandle,
    pub(crate) recent_preview: Vec<(usize, usize)>,
    closed_projects: BTreeSet<usize>,
    closed_live_projects: BTreeSet<(sailry_protocol::NodeId, sailry_protocol::ProjectId)>,
    pub(crate) hovered: Option<Row>,
    branches: std::collections::BTreeMap<
        (sailry_protocol::NodeId, sailry_protocol::WorktreeId),
        sessions::Branch,
    >,
}

impl State {
    pub(crate) fn group_open(&self, target: crate::panes::Target) -> bool {
        !self.closed_groups.contains(&target)
    }
    pub(crate) fn toggle_group(&mut self, target: crate::panes::Target) {
        if !self.closed_groups.remove(&target) {
            self.closed_groups.insert(target);
        }
    }
    pub(crate) fn open_group(&mut self, target: crate::panes::Target) {
        self.closed_groups.remove(&target);
    }

    pub(crate) fn project_open(
        &self,
        node: sailry_protocol::NodeId,
        project: sailry_protocol::ProjectId,
    ) -> bool {
        !self.closed_live_projects.contains(&(node, project))
    }
    pub(crate) fn toggle_project(
        &mut self,
        node: sailry_protocol::NodeId,
        project: sailry_protocol::ProjectId,
    ) {
        if !self.closed_live_projects.remove(&(node, project)) {
            self.closed_live_projects.insert((node, project));
        }
    }
    pub(crate) fn open_project(&mut self, project: usize) {
        self.closed_projects.remove(&project);
    }
}

// Kit ListItem has no branch guides; decorate the existing rows without changing
// their focus, drag or menu behavior, matching the database tree's guide style.
pub(crate) fn branch(id: String, last: bool, indent: Pixels, cx: &App) -> Div {
    div()
        .debug_selector(move || format!("{id}-guide"))
        .absolute()
        .left(px(16.) - indent)
        .top(px(-4.))
        .bottom(px(-4.))
        .w(indent - px(18.))
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .h(if last { relative(0.5) } else { relative(1.) })
                .border_l_1()
                .border_dashed()
                .border_color(cx.theme().sidebar_foreground.opacity(0.35)),
        )
        .child(
            div()
                .absolute()
                .top(relative(0.5))
                .w_full()
                .border_t_1()
                .border_dashed()
                .border_color(cx.theme().sidebar_foreground.opacity(0.35)),
        )
}

fn location(id: String, label: SharedString, cx: &App) -> impl IntoElement {
    h_flex()
        .max_w_20()
        .min_w_0()
        .flex_shrink_0()
        .gap_1()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .debug_selector(move || id.clone())
        .child(Icon::new(IconName::Network).size_3().flex_shrink_0())
        .child(div().min_w_0().truncate().child(label))
}

impl Shell {
    // Own the sidebar composition so host rows and hover-only session details can
    // share a fixed footer and native material without SidebarMenu's row constraints.
    pub(super) fn sidebar(&self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.page == Page::Settings {
            return self.settings_sidebar(cx).into_any_element();
        }
        let navigation = Button::new("new-conversation")
            .primary()
            .w_full()
            .label(tr("conversation"))
            .icon(IconName::Plus)
            .debug_selector(|| "new-conversation".into())
            .on_click(cx.listener(|this, _, window, cx| {
                this.start_conversation(window, cx);
            }));
        v_flex()
            .size_full()
            .text_color(cx.theme().sidebar_foreground)
            .child(div().flex_shrink_0().px_3().pt_4().pb_2().child(navigation))
            .child(
                v_flex().flex_1().min_h_0().child(
                    v_flex()
                        .id("sidebar-resources")
                        .flex_1()
                        .min_h_0()
                        .px_3()
                        .py_2()
                        .gap_1()
                        .child(
                            Collapsible::new()
                                .flex_shrink_0()
                                .open(!self.sidebar.hosts_closed)
                                .child(
                                    h_flex()
                                        .id("hosts-heading")
                                        .debug_selector(|| "hosts-heading".into())
                                        .h_8()
                                        .flex_shrink_0()
                                        .px_2()
                                        .gap_1()
                                        .text_sm()
                                        .text_color(cx.theme().sidebar_foreground.opacity(0.5))
                                        .on_hover(cx.listener(|shell, hovered, _, cx| {
                                            if *hovered {
                                                shell.sidebar.hovered = Some(Row::Hosts);
                                            } else if shell.sidebar.hovered == Some(Row::Hosts) {
                                                shell.sidebar.hovered = None;
                                            }
                                            cx.notify();
                                        }))
                                        .on_click(cx.listener(|shell, _, _, cx| {
                                            shell.sidebar.hosts_closed =
                                                !shell.sidebar.hosts_closed;
                                            cx.notify();
                                        }))
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .debug_selector(|| "hosts-label".into())
                                                .child(tr("hosts")),
                                        )
                                        .child(div().flex_1())
                                        .child(
                                            Button::new("host-add")
                                                .ghost()
                                                .xsmall()
                                                .icon(
                                                    Icon::new(IconName::Plus)
                                                        .size(px(14.))
                                                        .text_color(
                                                            cx.theme()
                                                                .sidebar_foreground
                                                                .opacity(0.5),
                                                        ),
                                                )
                                                .when(
                                                    self.sidebar.hovered != Some(Row::Hosts),
                                                    |button| {
                                                        button
                                                            .opacity(0.)
                                                            .focus(|style| style.opacity(1.))
                                                    },
                                                )
                                                .tooltip(tr("host_add"))
                                                .accessibility_label(tr("host_add"))
                                                .debug_selector(|| "host-add".into())
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    cx.stop_propagation();
                                                    this.add_host(window, cx);
                                                })),
                                        )
                                        .child(
                                            gpui_kit::base::AccordionTrigger::new("hosts-toggle")
                                                .open(!self.sidebar.hosts_closed)
                                                .tab_index(0)
                                                .h_flex()
                                                .h_8()
                                                .flex_shrink_0()
                                                .justify_end()
                                                .aria_label(tr("hosts"))
                                                .debug_selector(|| "hosts-toggle".into())
                                                .child(
                                                    div()
                                                        .flex()
                                                        .w_5()
                                                        .items_center()
                                                        .justify_center()
                                                        .debug_selector(|| {
                                                            "hosts-disclosure".into()
                                                        })
                                                        .child(
                                                            Icon::new(
                                                                if self.sidebar.hosts_closed {
                                                                    IconName::ChevronRight
                                                                } else {
                                                                    IconName::ChevronDown
                                                                },
                                                            )
                                                            .size(px(12.))
                                                            .when(
                                                                self.sidebar.hovered
                                                                    != Some(Row::Hosts),
                                                                |icon| icon.opacity(0.),
                                                            ),
                                                        ),
                                                )
                                                .on_change({
                                                    let shell = cx.entity().downgrade();
                                                    move |open, _, _, cx| {
                                                        cx.stop_propagation();
                                                        let _ = shell.update(cx, |shell, cx| {
                                                            shell.sidebar.hosts_closed = !open;
                                                            cx.notify();
                                                        });
                                                    }
                                                }),
                                        ),
                                )
                                .content(self.host_rows(cx)),
                        )
                        .child(self.active_group(cx))
                        .child(self.unassigned_sessions(cx))
                        .child(
                            Collapsible::new()
                                .min_h_8()
                                .when(self.sidebar.projects_closed, |group| group.flex_shrink_0())
                                .open(!self.sidebar.projects_closed)
                                .child(
                                    h_flex()
                                        .id("projects-heading")
                                        .debug_selector(|| "projects-heading".into())
                                        .h_8()
                                        .flex_shrink_0()
                                        .px_2()
                                        .gap_1()
                                        .text_sm()
                                        .text_color(cx.theme().sidebar_foreground.opacity(0.5))
                                        .on_hover(cx.listener(|shell, hovered, _, cx| {
                                            if *hovered {
                                                shell.sidebar.hovered = Some(Row::Projects);
                                            } else if shell.sidebar.hovered == Some(Row::Projects) {
                                                shell.sidebar.hovered = None;
                                            }
                                            cx.notify();
                                        }))
                                        .on_click(cx.listener(|shell, _, _, cx| {
                                            shell.sidebar.projects_closed =
                                                !shell.sidebar.projects_closed;
                                            cx.notify();
                                        }))
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .debug_selector(|| "projects-label".into())
                                                .child(tr("projects")),
                                        )
                                        .child(div().flex_1())
                                        .child(
                                            Button::new("project-add")
                                                .ghost()
                                                .xsmall()
                                                .icon(
                                                    Icon::new(IconName::Plus)
                                                        .size(px(14.))
                                                        .text_color(
                                                            cx.theme()
                                                                .sidebar_foreground
                                                                .opacity(0.5),
                                                        ),
                                                )
                                                .when(
                                                    self.sidebar.hovered != Some(Row::Projects),
                                                    |button| {
                                                        button
                                                            .opacity(0.)
                                                            .focus(|style| style.opacity(1.))
                                                    },
                                                )
                                                .tooltip(tr("project_add"))
                                                .accessibility_label(tr("project_add"))
                                                .debug_selector(|| "project-add".into())
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    cx.stop_propagation();
                                                    this.project_editor(
                                                        this.host, None, window, cx,
                                                    );
                                                })),
                                        )
                                        .child(
                                            gpui_kit::base::AccordionTrigger::new(
                                                "projects-toggle",
                                            )
                                            .open(!self.sidebar.projects_closed)
                                            .tab_index(0)
                                            .h_flex()
                                            .h_8()
                                            .flex_shrink_0()
                                            .justify_end()
                                            .aria_label(tr("projects"))
                                            .debug_selector(|| "projects-toggle".into())
                                            .child(
                                                div()
                                                    .flex()
                                                    .w_5()
                                                    .items_center()
                                                    .justify_center()
                                                    .debug_selector(|| "projects-disclosure".into())
                                                    .child(
                                                        Icon::new(
                                                            if self.sidebar.projects_closed {
                                                                IconName::ChevronRight
                                                            } else {
                                                                IconName::ChevronDown
                                                            },
                                                        )
                                                        .size(px(12.))
                                                        .when(
                                                            self.sidebar.hovered
                                                                != Some(Row::Projects),
                                                            |icon| icon.opacity(0.),
                                                        ),
                                                    ),
                                            )
                                            .on_change({
                                                let shell = cx.entity().downgrade();
                                                move |open, _, _, cx| {
                                                    cx.stop_propagation();
                                                    let _ = shell.update(cx, |shell, cx| {
                                                        shell.sidebar.projects_closed = !open;
                                                        cx.notify();
                                                    });
                                                }
                                            }),
                                        ),
                                )
                                .content(
                                    v_flex()
                                        .id("sidebar-projects")
                                        .relative()
                                        .h_auto()
                                        .min_h_0()
                                        .debug_selector(|| "sidebar-projects-viewport".into())
                                        .child(v_flex().flex_none().child(self.project_rows(cx)))
                                        .overflow_y_scroll()
                                        .track_scroll(&self.sidebar.projects_scroll)
                                        .vertical_scrollbar(&self.sidebar.projects_scroll),
                                ),
                        )
                        .child(self.recent_sessions(cx)),
                ),
            )
            .child(self.sidebar_footer(cx))
            .into_any_element()
    }

    pub(crate) fn sidebar_row(
        &self,
        id: impl Into<ElementId>,
        row: Row,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> ListItem {
        let hovered = self.sidebar.hovered == Some(row);
        let project = matches!(row, Row::Project(_) | Row::LiveProject(_));
        let muted = matches!(
            row,
            Row::Session(..)
                | Row::Terminal(..)
                | Row::LiveSession(_)
                | Row::LiveTerminal(_)
                | Row::RecentSession(_)
                | Row::RecentPreview(..)
                | Row::ActiveSession(_)
        ) && !selected
            && !hovered;
        ListItem::new(id)
            .w_full()
            .relative()
            .px_2()
            .py_1()
            .text_sm()
            .rounded(cx.theme().radius)
            // Confirmed suppresses native hover without the secondary-selection outline.
            .confirmed(selected || hovered || project)
            .bg(theme::sidebar_item(selected, hovered && !project, cx))
            .text_color(if muted {
                cx.theme().muted_foreground
            } else {
                cx.theme().sidebar_foreground
            })
            .on_hover(cx.listener(move |this, hovered, _, cx| {
                if *hovered {
                    if this.sidebar.hovered == Some(row) {
                        return;
                    }
                    this.sidebar.hovered = Some(row);
                    if let Row::LiveSession(id) | Row::RecentSession(id) | Row::ActiveSession(id) =
                        row
                    {
                        this.load_sidebar_branch(id, cx);
                    }
                } else if this.sidebar.hovered == Some(row) {
                    this.sidebar.hovered = None;
                }
                cx.notify();
            }))
    }

    fn group_title(key: &str, cx: &App) -> impl IntoElement {
        h_flex()
            .h_8()
            .px_2()
            .flex_shrink_0()
            .text_sm()
            .text_color(cx.theme().sidebar_foreground.opacity(0.5))
            .child(tr(key))
    }

    fn host_rows(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.live.is_some() {
            return self.live_host_rows(cx).into_any_element();
        }
        v_flex()
            .gap_0p5()
            .children(
                ["local_host_name", "remote_host_name"]
                    .into_iter()
                    .enumerate()
                    .map(|(host, name)| {
                        self.sidebar_row(("host", host), Row::Host(host), self.host == host, cx)
                            .debug_selector(move || format!("host-{host}"))
                            .tooltip(move |window, cx| Tooltip::new(tr(name)).build(window, cx))
                            .child(
                                h_flex().gap_2().child(Icon::new(IconName::Cpu)).child(
                                    div()
                                        .debug_selector(move || format!("host-name-{host}"))
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .child(tr(name)),
                                ),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_host(host, window, cx)
                            }))
                    }),
            )
            .into_any_element()
    }

    fn sidebar_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let live = self.live.is_some();
        v_flex()
            .debug_selector(|| "sidebar-footer".into())
            .flex_shrink_0()
            .px_3()
            .child(Separator::horizontal().w_auto().mx_neg_3())
            .child(
                h_flex().py_2().items_center().justify_between().child(
                    h_flex().gap_3().children(
                        [
                            ("cpu-preview", "metrics_cpu", 55.),
                            ("memory-preview", "metrics_memory", 41.),
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(index, (id, label, preview))| {
                            let value = if live {
                                self.host_monitor
                                    .as_ref()
                                    .and_then(|monitor| monitor.read(cx).percentages()[index])
                            } else {
                                Some(preview)
                            };
                            let value = value.map(|value| value.clamp(0., 100.));
                            let text = value
                                .map(|value| format!("{value:.0}%"))
                                .unwrap_or_else(|| "—".into());
                            let tooltip = format!("{} · {text}", tr(label));
                            div()
                                .id(id)
                                .debug_selector(move || id.into())
                                .w_16()
                                .tooltip(move |window, cx| {
                                    Tooltip::new(tooltip.clone()).build(window, cx)
                                })
                                .child(
                                    v_flex()
                                        .gap_1()
                                        .child(
                                            h_flex()
                                                .justify_between()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(tr(label))
                                                .child(
                                                    div()
                                                        .debug_selector(move || {
                                                            format!("{id}-value")
                                                        })
                                                        .child(text),
                                                ),
                                        )
                                        .child(
                                            Progress::new((id, 0usize))
                                                .xsmall()
                                                .value(value.unwrap_or(0.))
                                                .accessibility_label(tr(label))
                                                .color(cx.theme().muted_foreground),
                                        ),
                                )
                        }),
                    ),
                ),
            )
    }
}
