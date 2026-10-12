use super::*;
use crate::theme::DialogStyle as _;
use gpui_kit::component::{WindowExt as _, button::Button, dialog::DialogFooter};

impl Queue {
    pub(super) fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .window
            .is_some_and(|id| !cx.windows().iter().any(|window| window.window_id() == id))
        {
            self.window = None;
            self.focus = None;
        }
        if self.entries.is_empty() || self.window.is_some() {
            return;
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
        let owner = cx.entity();
        window.open_dialog(cx, move |dialog, window, cx| {
            let close = owner.clone();
            let hide = owner.clone();
            let list = owner
                .read(cx)
                .list
                .as_ref()
                .expect("installation list is mounted")
                .clone();
            dialog
                .form_title(tr("installation_title"))
                .w((window.viewport_size().width - px(48.)).min(px(600.)))
                .overlay_closable(false)
                .on_close(move |_, _, cx| {
                    close.update(cx, |queue, cx| {
                        queue.focus = None;
                        queue.window = None;
                        cx.notify();
                    })
                })
                .child(
                    div()
                        .debug_selector(|| "installation-list".into())
                        .h((window.viewport_size().height * 0.55).min(px(420.)))
                        .child(List::new(&list).h_full()),
                )
                .footer(
                    DialogFooter::new().child(
                        Button::new("installation-hide")
                            .label(tr("installation_hide"))
                            .debug_selector(|| "installation-hide".into())
                            .on_click(move |_, window, cx| {
                                hide.update(cx, |queue, cx| queue.hide(window, cx))
                            }),
                    ),
                )
        });
        self.window = Some(window.window_handle().window_id());
        self.focus = window.focused(cx);
        if let Some(focus) = &self.focus {
            cx.on_focus_in(focus, window, |queue, window, cx| {
                if queue.entries.is_empty() {
                    cx.defer_in(window, |queue, window, cx| queue.close_empty(window, cx));
                }
            })
            .detach();
        }
    }

    pub(super) fn close_empty(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.entries.is_empty() {
            self.hide(window, cx);
        }
    }

    pub(super) fn hide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Kit closes the top dialog only. Never dismiss a form or confirmation
        // stacked above this one; clean up when this dialog regains focus.
        if self.window == Some(window.window_handle().window_id())
            && self
                .focus
                .as_ref()
                .is_some_and(|focus| focus.contains_focused(window, cx))
        {
            self.focus = None;
            self.window = None;
            window.close_dialog(cx);
        }
    }
}
