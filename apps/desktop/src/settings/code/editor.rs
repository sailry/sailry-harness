use super::*;
use crate::{theme::DialogStyle as _, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    dialog::DialogFooter,
    form::Field,
    input::{Input, InputState},
    scroll::ScrollableElement,
};

struct Editor {
    owner: Entity<Panel>,
    server: String,
    fields: Vec<(&'static str, Entity<InputState>)>,
    error: Option<&'static str>,
}

pub(super) fn open(owner: Entity<Panel>, server: String, window: &mut Window, cx: &mut App) {
    let current = owner
        .read(cx)
        .draft
        .as_ref()
        .and_then(|draft| draft.servers.get(&server))
        .cloned();
    let values = current
        .map(|value| {
            [
                value.command.unwrap_or_default(),
                value
                    .args
                    .map(|value| serde_json::to_string(&value).expect("argument serialization"))
                    .unwrap_or_default(),
                value
                    .config
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                value
                    .environment
                    .map(|value| serde_json::to_string(&value).expect("environment serialization"))
                    .unwrap_or_default(),
            ]
        })
        .unwrap_or_default();
    let editor = cx.new(|cx| {
        crate::feedback::observe(window, cx, |editor: &Editor, _| {
            editor.error.into_iter().collect()
        });
        let fields = [
            "code_command",
            "code_arguments",
            "code_configuration",
            "code_environment",
        ]
        .into_iter()
        .zip(values)
        .map(|(key, value)| {
            (
                key,
                cx.new(|cx| InputState::new(window, cx).default_value(value)),
            )
        })
        .collect();
        Editor {
            owner,
            server,
            fields,
            error: None,
        }
    });
    window.open_dialog(cx, move |dialog, window, _| {
        let save = editor.clone();
        dialog
            .form_title(tr("code_custom_command"))
            .w((window.viewport_size().width - px(48.)).min(px(600.)))
            .child(editor.clone())
            .footer(
                DialogFooter::new()
                    .w_full()
                    .gap_2()
                    .child(
                        Button::new("code-editor-cancel")
                            .label(tr("settings_cancel"))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("code-editor-save")
                            .primary()
                            .label(tr("settings_save"))
                            .debug_selector(|| "code-editor-save".into())
                            .on_click(move |_, window, cx| {
                                save.update(cx, |editor, cx| editor.save(window, cx))
                            }),
                    ),
            )
    });
}

impl Editor {
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let values: Vec<_> = self
            .fields
            .iter()
            .map(|(_, input)| input.read(cx).value().trim().to_owned())
            .collect();
        let parsed = (|| {
            Ok::<_, serde_json::Error>((
                (!values[1].is_empty())
                    .then(|| serde_json::from_str::<Vec<String>>(&values[1]))
                    .transpose()?,
                (!values[2].is_empty())
                    .then(|| serde_json::from_str::<serde_json::Value>(&values[2]))
                    .transpose()?,
                (!values[3].is_empty())
                    .then(|| serde_json::from_str::<BTreeMap<String, String>>(&values[3]))
                    .transpose()?,
            ))
        })();
        let Ok((args, config, environment)) = parsed else {
            self.error = Some("code_invalid");
            cx.notify();
            return;
        };
        let applied = self.owner.update(cx, |panel, cx| {
            if !panel.active || panel.locked() {
                panel.error = Some("code_target_changed");
                cx.notify();
                return false;
            }
            let Some(draft) = panel.draft.as_mut() else {
                return false;
            };
            let disabled = draft
                .servers
                .get(&self.server)
                .is_some_and(|value| value.disabled);
            draft.servers.insert(
                self.server.clone(),
                code::Override {
                    disabled,
                    command: (!values[0].is_empty()).then(|| values[0].clone()),
                    args,
                    config,
                    environment,
                },
            );
            panel.save(window, cx);
            true
        });
        if applied {
            window.close_dialog(cx);
        }
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("code-command-editor")
            .gap_4()
            .max_h((window.viewport_size().height - px(230.)).max(px(160.)))
            .overflow_y_scrollbar()
            .children(self.fields.iter().enumerate().map(|(index, (key, state))| {
                Field::new().label(tr(key)).child(
                    div()
                        .debug_selector(move || format!("code-command-{index}"))
                        .child(Input::new(state).aria_label(tr(key))),
                )
            }))
    }
}
