use super::*;
use crate::theme::DialogStyle as _;

pub(super) fn open(editor: Entity<Editor>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, window, cx| {
        let confirm = editor.clone();
        let save = editor.clone();
        let close = editor.clone();
        let cancel = editor.clone();
        let reload = editor.clone();
        let details = editor.clone();
        let state = editor.read(cx);
        let title = if state.panel {
            state.info.as_ref().map(crate::plugins::metadata::title)
        } else {
            None
        };
        dialog
            .form_title(
                title
                    .map(SharedString::from)
                    .unwrap_or_else(|| tr("plugins_settings_title")),
            )
            .w((window.viewport_size().width - px(48.)).min(px(500.)))
            .max_h(window.viewport_size().height * 0.8)
            .on_ok(move |_, window, cx| {
                confirm.update(cx, |editor, cx| editor.save(window, cx));
                false
            })
            .on_close(move |_, window, cx| close.update(cx, |editor, cx| editor.close(window, cx)))
            .child(editor.clone())
            .footer(
                v_flex()
                    .w_full()
                    .gap_2()
                    .when(state.changed, |footer| {
                        footer.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(tr("plugins_settings_changed")),
                        )
                    })
                    .when(state.details, |footer| {
                        footer.children(
                            state.detail.clone().map(|text| {
                                div().max_h_16().overflow_hidden().text_xs().child(text)
                            }),
                        )
                    })
                    .child(
                        gpui_kit::component::dialog::DialogFooter::new()
                            .w_full()
                            .gap_2()
                            .flex_wrap()
                            .when(state.detail.is_some(), |row| {
                                row.child(
                                    Button::new("plugin-settings-details")
                                        .ghost()
                                        .label(tr("plugins_settings_diagnostics"))
                                        .on_click(move |_, _, cx| {
                                            details.update(cx, |editor, cx| {
                                                editor.details = !editor.details;
                                                cx.notify();
                                            })
                                        }),
                                )
                            })
                            .when(!state.panel, |row| {
                                row.child(
                                    Button::new("plugin-settings-reload")
                                        .debug_selector(|| "plugin-settings-reload".into())
                                        .label(tr("plugins_settings_reload"))
                                        .disabled(
                                            state.pending
                                                || state.loading
                                                || state.request.is_some(),
                                        )
                                        .on_click(move |_, window, cx| {
                                            reload.update(cx, |editor, cx| {
                                                editor.load(true, window, cx)
                                            })
                                        }),
                                )
                            })
                            .child(
                                Button::new("plugin-settings-cancel")
                                    .debug_selector(|| "plugin-settings-cancel".into())
                                    .label(tr("settings_cancel"))
                                    .on_click(move |_, window, cx| {
                                        cancel.update(cx, |editor, cx| editor.close(window, cx));
                                        window.close_dialog(cx);
                                    }),
                            )
                            .child(
                                Button::new("plugin-settings-save")
                                    .debug_selector(|| "plugin-settings-save".into())
                                    .primary()
                                    .disabled(
                                        state.pending || state.loading || state.info.is_none(),
                                    )
                                    .label(tr(if state.request.is_some() {
                                        "plugins_retry"
                                    } else {
                                        "settings_save"
                                    }))
                                    .on_click(move |_, window, cx| {
                                        save.update(cx, |editor, cx| editor.save(window, cx))
                                    }),
                            ),
                    ),
            )
    });
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("plugin-settings-form")
            .debug_selector(|| "plugin-settings-form".into())
            .gap_4()
            .min_w_0()
            .when(self.loading, |view| view.child(tr("plugins_loading")))
            .children(self.tab_bar(cx))
            .child(
                gpui_kit::component::form::Form::vertical()
                    .gap_4()
                    .children(
                        self.visible_fields(cx)
                            .iter()
                            .map(|field| Setting::field(field, cx)),
                    ),
            )
            .when(self.embedded, |view| {
                view.child(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("plugin-settings-reload")
                                .label(tr("plugins_settings_reload"))
                                .disabled(self.pending || self.loading || self.request.is_some())
                                .on_click(cx.listener(|editor, _, window, cx| {
                                    editor.load(true, window, cx)
                                })),
                        )
                        .child(
                            Button::new("plugin-settings-save")
                                .primary()
                                .label(tr(if self.request.is_some() {
                                    "plugins_retry"
                                } else {
                                    "settings_save"
                                }))
                                .debug_selector(|| "plugin-settings-save".into())
                                .disabled(self.pending || self.loading || self.info.is_none())
                                .on_click(
                                    cx.listener(|editor, _, window, cx| editor.save(window, cx)),
                                ),
                        ),
                )
            })
    }
}
