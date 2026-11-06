//! Shared resource navigation composes the approved sidebar row.
use super::*;
use crate::sidebar::Row;
use gpui_kit::{component::*, prelude::FluentBuilder as _};

impl Shell {
    pub(crate) fn live_terminal_row(
        &self,
        project: Option<ProjectId>,
        _index: usize,
        info: &Info,
        entry: &crate::sidebar::tree::Entry,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let live = self.live.as_ref().unwrap();
        let info = info.clone();
        let id = info.id;
        let node = live.selected;
        let activity = sailry_client::activity::terminal_activity(&info);
        let running = activity == Some(sailry_client::activity::Lane::Running);
        let title = self.terminal_title(node, info.worktree.unwrap(), id, cx);
        let accessible: SharedString = if running {
            format!("{title} · {}", tr("activity_running")).into()
        } else {
            title.clone()
        };
        let row = self
            .sidebar_row(
                SharedString::from(format!("live-terminal-{id}")),
                Row::LiveTerminal(id),
                self.page == Page::Terminal
                    && live.project == project
                    && info
                        .worktree
                        .and_then(|tree| self.terminals.selected.get(&(live.selected, tree)))
                        == Some(&id),
                cx,
            )
            .accessibility_label(accessible)
            .pl(entry.indent())
            .when(running, |row| row.text_color(cx.theme().sidebar_foreground))
            .relative()
            .child(entry.guides(
                format!("live-terminal-{id}"),
                running.then(|| {
                    crate::ui::loading::mini()
                        .debug_selector(move || format!("live-terminal-{id}-loading"))
                        .into_any_element()
                }),
                cx,
            ))
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
            .debug_selector(move || format!("live-terminal-{id}"))
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
                            .debug_selector(move || format!("live-terminal-{id}-label"))
                            .child(title),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .debug_selector(move || format!("live-terminal-{id}-icon"))
                            .child(
                                Icon::new(IconName::SquareTerminal)
                                    .size_4()
                                    .text_color(cx.theme().muted_foreground),
                            ),
                    ),
            );
        self.split_destination(row, entry, cx)
            .on_click(cx.listener(move |shell, _, window, cx| {
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
