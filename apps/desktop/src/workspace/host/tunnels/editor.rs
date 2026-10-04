use super::*;
use crate::theme::DialogStyle as _;
use gpui_kit::component::{
    form::field,
    input::{Input, InputState},
};

struct Editor {
    shell: WeakEntity<Shell>,
    host: usize,
    original: Vec<u16>,
    input: Entity<InputState>,
    error: Option<&'static str>,
}

impl Shell {
    pub(crate) fn tunnel_editor(&self, host: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.workspace.tunnels.get(&host) else {
            return;
        };
        if !state.editable || !state.loaded || state.error.is_some() {
            return;
        }
        let shell = cx.entity().downgrade();
        let original = state.ports.clone();
        let editor = cx.new(|cx| Editor {
            shell,
            host,
            input: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(ports_label(&original))
                    .placeholder(tr("tunnel_ports_placeholder"))
            }),
            original,
            error: None,
        });
        editor.update(cx, |_, cx| {
            crate::feedback::observe(window, cx, |view, _| view.error.into_iter().collect());
        });
        window.open_dialog(cx, move |dialog, window, _| {
            let save = editor.clone();
            let confirm = editor.clone();
            dialog
                .form_title(tr("tunnel_edit"))
                .w((window.viewport_size().width - px(48.)).min(px(480.)))
                .overlay_closable(false)
                .on_ok(move |_, window, cx| {
                    confirm.update(cx, |this, cx| this.save(window, cx));
                    false
                })
                .child(editor.clone())
                .footer(
                    gpui_kit::component::dialog::DialogFooter::new()
                        .w_full()
                        .gap_2()
                        .child(
                            Button::new("tunnel-policy-cancel")
                                .label(tr("settings_cancel"))
                                .debug_selector(|| "tunnel-policy-cancel".into())
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("tunnel-policy-save")
                                .primary()
                                .label(tr("settings_save_preview"))
                                .debug_selector(|| "tunnel-policy-save".into())
                                .on_click(move |_, window, cx| {
                                    save.update(cx, |this, cx| this.save(window, cx))
                                }),
                        ),
                )
        });
    }
}

impl Editor {
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.input.read(cx).value();
        let result = self
            .shell
            .update(cx, |shell, cx| {
                shell
                    .workspace
                    .tunnels
                    .get_mut(&self.host)
                    .ok_or("tunnel_policy_changed")?
                    .save_ports(&self.original, &value)?;
                cx.notify();
                Ok(())
            })
            .unwrap_or(Err("tunnel_policy_changed"));
        match result {
            Ok(()) => window.close_dialog(cx),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_3()
            .debug_selector(|| "tunnel-policy-editor".into())
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("tunnel_policy_hint")),
            )
            .child(
                gpui_kit::component::form::Form::vertical().child(
                    field().label(tr("tunnel_ports_label")).child(
                        div()
                            .debug_selector(|| "tunnel-policy-input".into())
                            .child(Input::new(&self.input).aria_label(tr("tunnel_ports_label"))),
                    ),
                ),
            )
    }
}
