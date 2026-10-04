use super::*;
use crate::theme::DialogStyle as _;
use gpui_kit::component::{dialog::DialogFooter, input::Editor as CodeEditor};

pub(super) fn open(editor: Entity<Editor>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, window, cx| {
        let save = editor.clone();
        let cancel = editor.clone();
        let close = editor.clone();
        let reload = editor.clone();
        let details = editor.clone();
        let state = editor.read(cx);
        dialog
            .form_title(tr("mcp_edit"))
            .w((window.viewport_size().width - px(48.)).min(px(520.)))
            .max_h(window.viewport_size().height * 0.85)
            .overlay_closable(false)
            .on_close(move |_, window, cx| close.update(cx, |editor, cx| editor.close(window, cx)))
            .child(editor.clone())
            .footer(
                v_flex()
                    .w_full()
                    .gap_2()
                    .when(state.details, |view| {
                        view.children(
                            state.detail.clone().map(|text| {
                                div().text_xs().max_h_16().overflow_hidden().child(text)
                            }),
                        )
                    })
                    .child(
                        DialogFooter::new()
                            .w_full()
                            .gap_2()
                            .flex_wrap()
                            .when(!state.loaded, |row| {
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
                            })
                            .when(state.detail.is_some(), |row| {
                                row.child(
                                    Button::new("mcp-configuration-diagnostics")
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
                            .child(
                                Button::new("mcp-configuration-cancel")
                                    .debug_selector(|| "mcp-configuration-cancel".into())
                                    .label(tr("settings_cancel"))
                                    .on_click(move |_, window, cx| {
                                        cancel.update(cx, |editor, cx| editor.close(window, cx));
                                        window.close_dialog(cx);
                                    }),
                            )
                            .child(
                                Button::new("mcp-configuration-save")
                                    .primary()
                                    .debug_selector(|| "mcp-configuration-save".into())
                                    .loading(state.pending)
                                    .disabled(state.pending || state.loading || !state.loaded)
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
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("mcp-configuration")
            .debug_selector(|| "mcp-configuration".into())
            .child(
                div()
                    .debug_selector(|| "mcp-configuration-document".into())
                    .child(
                        CodeEditor::new(&self.document)
                            .h(px(360.).min(window.viewport_size().height * 0.55))
                            .readonly(self.locked())
                            .aria_label(tr("mcp_configuration")),
                    ),
            )
    }
}
