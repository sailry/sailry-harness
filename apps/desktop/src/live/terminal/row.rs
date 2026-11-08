//! Shared resource navigation composes the approved sidebar row.
use super::*;
use crate::sidebar::Row;
use gpui_kit::{component::*, prelude::FluentBuilder as _};

pub(crate) enum Placement<'a> {
    Project(&'a crate::sidebar::tree::Entry),
    Pinned { last: bool },
}

impl Shell {
    pub(crate) fn live_terminal_row(
        &self,
        project: Option<ProjectId>,
        info: &Info,
        placement: Placement<'_>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let live = self.live.as_ref().unwrap();
        let info = info.clone();
        let id = info.id;
        let node = live.selected;
        let (prefix, row_key, entry, last) = match placement {
            Placement::Project(entry) => (
                "live-terminal",
                Row::LiveTerminal(id),
                Some(entry),
                entry.last,
            ),
            Placement::Pinned { last } => ("pinned-terminal", Row::PinnedTerminal(id), None, last),
        };
        let project_name = matches!(placement, Placement::Pinned { .. })
            .then(|| {
                let snapshot = live.view.snapshot.as_ref()?;
                snapshot
                    .projects
                    .iter()
                    .find(|value| Some(value.id) == project)
                    .map(|project| project.name.clone())
            })
            .flatten();
        let activity = sailry_client::activity::terminal_activity(&info);
        let running = activity == Some(sailry_client::activity::Lane::Running);
        let title = self.terminal_title(node, info.worktree.unwrap(), id, cx);
        let accessible: SharedString = if running {
            format!("{title} · {}", tr("activity_running")).into()
        } else {
            title.clone()
        };
        let status = running.then(|| {
            crate::ui::loading::small()
                .debug_selector(move || format!("{prefix}-{id}-loading"))
                .into_any_element()
        });
        let guides = match entry {
            Some(entry) => entry.guides(format!("{prefix}-{id}"), status, cx),
            None => crate::sidebar::branch(format!("{prefix}-{id}"), last, px(30.), status, cx),
        };
        let row = self
            .sidebar_row(
                SharedString::from(format!("{prefix}-{id}")),
                row_key,
                self.page == Page::Terminal
                    && live.project == project
                    && info
                        .worktree
                        .and_then(|tree| self.terminals.selected.get(&(live.selected, tree)))
                        == Some(&id),
                cx,
            )
            .accessibility_label(accessible)
            .pl(entry.map_or(px(30.), |entry| entry.indent()))
            .when(running, |row| row.text_color(cx.theme().sidebar_foreground))
            .relative()
            .child(guides)
            .on_drag(
                crate::panes::Drag {
                    target: crate::panes::Target::Terminal(node, info.worktree.unwrap(), id),
                    session: None,
                    terminal: Some(info.clone()),
                    project,
                    title: title.clone(),
                }
                .payload(),
                |drag, _, _, cx| {
                    cx.new(|_| {
                        drag.value()
                            .downcast_ref::<crate::panes::Drag>()
                            .unwrap()
                            .clone()
                    })
                },
            )
            .debug_selector(move || format!("{prefix}-{id}"))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |shell, event, window, cx| {
                    shell.live_resource_menu(
                        node,
                        super::menus::Target::Terminal(id),
                        event,
                        window,
                        cx,
                    );
                }),
            )
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .h_5()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .debug_selector(move || format!("{prefix}-{id}-label"))
                            .child(title),
                    )
                    .when_some(project_name, |row, name| {
                        row.child(
                            div()
                                .min_w_0()
                                .max_w_20()
                                .truncate()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(name),
                        )
                    })
                    .child(
                        div()
                            .flex_shrink_0()
                            .debug_selector(move || format!("{prefix}-{id}-icon"))
                            .child(
                                Icon::new(IconName::SquareTerminal)
                                    .size_4()
                                    .text_color(cx.theme().muted_foreground),
                            ),
                    ),
            );
        let row = match entry {
            Some(entry) => self.split_destination(row, entry, cx),
            None => row,
        };
        row.on_click(cx.listener(move |shell, _, window, cx| {
            let guarded = info.clone();
            if shell.guard_file_navigation(window, cx, move |shell, window, cx| {
                shell.reveal_terminal(project, &guarded, window, cx)
            }) {
                return;
            }
            shell.reveal_terminal(project, &info, window, cx);
        }))
        .into_any_element()
    }
}
