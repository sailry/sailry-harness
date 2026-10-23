use super::*;
use crate::theme::DialogStyle as _;
use gpui_kit::component::{Size, dialog::DialogFooter, input::Editor as CodeEditor};

pub(super) fn open(editor: Entity<Editor>, window: &mut Window, cx: &mut App) {
    let active = editor.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let save = editor.clone();
        let cancel = editor.clone();
        let close = editor.clone();
        let reload = editor.clone();
        let details = editor.clone();
        let state = editor.read(cx);
        dialog
            .form_title(tr("mcp_edit"), cx)
            .w((window.viewport_size().width - px(48.)).min(px(520.)))
            .max_h(window.viewport_size().height * 0.85)
            .overlay_closable(false)
            .on_close(move |_, window, cx| {
                close.update(cx, |editor, cx| {
                    editor.close(window, cx);
                    editor.dialog.dismissed();
                })
            })
            .child(editor.clone())
            .footer(
                state.submission.diagnostics().child(
                    DialogFooter::new()
                        .w_full()
                        .gap_2()
                        .flex_wrap()
                        .when(
                            (!state.loaded || state.submission.error.is_some())
                                && state.submission.request.is_none(),
                            |row| {
                                row.child(
                                    Button::new("mcp-configuration-reload")
                                        .debug_selector(|| "mcp-configuration-reload".into())
                                        .label(tr("plugins_settings_reload"))
                                        .loading(state.loading)
                                        .disabled(state.loading)
                                        .on_click(move |_, window, cx| {
                                            reload.update(cx, |editor, cx| editor.load(window, cx))
                                        }),
                                )
                            },
                        )
                        .when(state.submission.detail.is_some(), |row| {
                            row.child(
                                Button::new("mcp-configuration-diagnostics")
                                    .ghost()
                                    .label(tr("plugins_settings_diagnostics"))
                                    .on_click(move |_, _, cx| {
                                        details.update(cx, |editor, cx| {
                                            editor.submission.details = !editor.submission.details;
                                            cx.notify();
                                        })
                                    }),
                            )
                        })
                        .child(
                            Button::new("mcp-configuration-cancel")
                                .debug_selector(|| "mcp-configuration-cancel".into())
                                .label(tr("settings_cancel"))
                                .on_click(move |_, window, cx| {
                                    cancel.update(cx, |editor, cx| {
                                        editor.close(window, cx);
                                        editor.dialog.dismissed();
                                    });
                                    window.close_dialog(cx);
                                }),
                        )
                        .child(
                            state
                                .submission
                                .save_button("mcp-configuration-save", state.loading, state.loaded)
                                .on_click(move |_, window, cx| {
                                    save.update(cx, |editor, cx| editor.save(window, cx))
                                }),
                        ),
                ),
            )
    });
    active.update(cx, |editor, cx| {
        crate::ui::overlays::Owned::bind(editor, |editor| &mut editor.dialog, window, cx)
    });
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let document = self.document.read(cx);
        // Kit's code editor exposes its measured rows; auto-grow is a textarea-only mode.
        let rows = document
            .display_map()
            .wrap_row_count()
            .max(document.text().len_lines(ropey::LineType::LF))
            .max(4);
        let line_height = document
            .line_height()
            .unwrap_or(cx.theme().mono_font_size * 1.5);
        let height = (line_height * rows as f32 + Size::default().input_py() * 2.)
            .min(window.viewport_size().height * 0.55);
        div()
            .id("mcp-configuration")
            .debug_selector(|| "mcp-configuration".into())
            .child(
                div()
                    .debug_selector(|| "mcp-configuration-document".into())
                    .child(
                        CodeEditor::new(&self.document)
                            .h(height)
                            .readonly(self.locked())
                            .aria_label(tr("mcp_configuration")),
                    ),
            )
    }
}
