use super::*;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Selectable as _,
    badge::Badge,
    button::{Button, ButtonVariants},
    popover::Popover,
    progress::ProgressCircle,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;

pub(crate) fn button(window: &mut Window, offset: Point<Pixels>, cx: &mut App) -> AnyElement {
    let owner = registry(cx);
    owner.update(cx, |queue, cx| queue.mount(window, cx));
    let queue = owner.read(cx);
    let open = queue.window == Some(window.window_handle().window_id());
    let attention = queue.entries.values().any(|entry| entry.status.terminal());
    let active: Vec<_> = queue
        .entries
        .values()
        .filter(|entry| {
            matches!(
                entry.status,
                Presentation::Starting | Presentation::Observed(Status::Running { .. })
            )
        })
        .collect();
    let measured: Vec<_> = active
        .iter()
        .filter_map(|entry| match &entry.status {
            Presentation::Observed(Status::Running {
                progress: Some(progress),
                ..
            }) if !matches!(
                progress,
                sailry_protocol::installation::Progress::Counts { total: 0, .. }
            ) =>
            {
                Some(rows::percentage(progress))
            }
            _ => None,
        })
        .collect();
    let value = if measured.is_empty() {
        0.
    } else {
        measured.iter().sum::<f32>() / measured.len() as f32
    };
    let active = !active.is_empty();
    let pending = visible(cx);
    let change = owner.downgrade();
    Popover::new("installations-popover")
        .anchor(Anchor::BottomLeft)
        .anchor_offset(offset)
        .open(open)
        .on_open_change(move |open, window, cx| {
            let _ = change.update(cx, |queue, cx| {
                if *open {
                    queue.open(window, cx);
                } else {
                    queue.hide(window, cx);
                }
            });
        })
        .w(px(320.).min(window.viewport_size().width - px(32.)))
        .p_2()
        .bg(cx.theme().title_bar)
        .trigger(
            Button::new("installations-open")
                .ghost()
                .size_9()
                .selected(open)
                .tooltip(tr("installation_title"))
                .accessibility_label(tr("installation_title"))
                .debug_selector(|| "installations-open".into())
                .child(
                    div()
                        .relative()
                        .when(active, |icon| {
                            icon.child(
                                ProgressCircle::new("installations-progress")
                                    .absolute()
                                    .inset_0()
                                    .size_8()
                                    .loading(measured.is_empty())
                                    .value(value)
                                    .color(cx.theme().foreground)
                                    .accessibility_label(tr("installation_title")),
                            )
                        })
                        .size_8()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(attention, |icon| {
                            icon.child(
                                Badge::new().dot().color(cx.theme().danger).child(
                                    Icon::new(gpui_kit::assets::IconName::Download).size_4(),
                                ),
                            )
                        })
                        .when(!attention, |icon| {
                            icon.child(Icon::new(gpui_kit::assets::IconName::Download).size_4())
                        })
                        .when(pending, |icon| {
                            icon.debug_selector(|| "installations-pending".into())
                        }),
                ),
        )
        .content(move |_, window, cx| {
            let queue = owner.read(cx);
            v_flex()
                .w_full()
                .debug_selector(|| "installation-panel".into())
                .gap_2()
                .child(
                    div()
                        .px_2()
                        .py_1()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("installation_title")),
                )
                .child(
                    div()
                        .debug_selector(|| "installation-list".into())
                        .when(queue.entries.is_empty(), |panel| {
                            panel.child(
                                div()
                                    .debug_selector(|| "installation-empty".into())
                                    .px_2()
                                    .py_5()
                                    .text_sm()
                                    .text_center()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(tr("composer_todo_empty")),
                            )
                        })
                        .when(!queue.entries.is_empty(), |panel| {
                            panel.child(
                                List::new(
                                    queue.list.as_ref().expect("installation list is mounted"),
                                )
                                .max_h((window.viewport_size().height * 0.55).min(px(400.))),
                            )
                        }),
                )
        })
        .into_any_element()
}

impl Queue {
    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .window
            .is_some_and(|id| !cx.windows().iter().any(|window| window.window_id() == id))
        {
            self.window = None;
        }
        if self.list.is_none() {
            let rows = rows::Rows {
                owner: cx.weak_entity(),
                entries: rows::snapshot(&self.entries),
            };
            self.list = Some(cx.new(|cx| {
                ListState::new(rows, window, cx)
                    .selectable(false)
                    .searchable(false)
            }));
        }
    }

    pub(super) fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mount(window, cx);
        if self.window == Some(window.window_handle().window_id()) {
            return;
        }
        self.window = Some(window.window_handle().window_id());
        self.changed(cx);
    }

    pub(super) fn close_empty(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.entries.is_empty() {
            self.hide(window, cx);
        }
    }

    pub(super) fn hide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.window == Some(window.window_handle().window_id()) {
            self.window = None;
            self.changed(cx);
        }
    }
}
