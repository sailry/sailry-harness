use super::*;
use crate::{theme::DialogStyle as _, tr};
use gpui_kit::component::{
    button::Button,
    dialog::DialogFooter,
    form::Field,
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement,
};

pub(super) struct Editor {
    owner: Entity<Panel>,
    server: String,
    pub(super) fields: Vec<(&'static str, Entity<InputState>)>,
    previous: Option<code::Override>,
    task: Option<Task<()>>,
    closed: bool,
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
        .clone()
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
        let fields: Vec<_> = [
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
        for (_, input) in &fields {
            cx.subscribe_in(
                input,
                window,
                |editor: &mut Editor, _, event, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        editor.changed(window, cx);
                    }
                },
            )
            .detach();
        }
        Editor {
            owner,
            server,
            fields,
            previous: current,
            task: None,
            closed: false,
            error: None,
        }
    });
    #[cfg(test)]
    {
        let owner = editor.read(cx).owner.clone();
        owner.update(cx, |panel, _| panel.editor = Some(editor.downgrade()));
    }
    window.open_dialog(cx, move |dialog, window, _| {
        let close = editor.clone();
        let cancel = editor.clone();
        dialog
            .form_title(tr("code_custom_command"))
            .w((window.viewport_size().width - px(48.)).min(px(600.)))
            .overlay_closable(false)
            .on_cancel(move |_, window, cx| {
                cancel.update(cx, |editor, cx| editor.finish(window, cx))
            })
            .child(editor.clone())
            .footer(
                DialogFooter::new().child(
                    Button::new("code-editor-close")
                        .label(tr("close"))
                        .debug_selector(|| "code-editor-close".into())
                        .on_click(move |_, window, cx| {
                            if close.update(cx, |editor, cx| editor.finish(window, cx)) {
                                window.close_dialog(cx);
                            }
                        }),
                ),
            )
    });
}

impl Editor {
    fn changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        self.error = None;
        self.task = Some(cx.spawn_in(window, async move |owner, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(350))
                .await;
            _ = owner.update_in(cx, |editor, window, cx| {
                editor.flush(false, window, cx);
            });
        }));
        cx.notify();
    }

    fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.owner.read(cx).active {
            self.error = Some("code_target_changed");
            self.closed = true;
            self.task = None;
            cx.notify();
            return true;
        }
        if self.flush(true, window, cx) {
            self.closed = true;
            self.task = None;
            true
        } else {
            false
        }
    }

    fn flush(&mut self, report: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
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
            if report {
                self.error = Some("code_invalid");
                cx.notify();
            }
            return false;
        };
        let script = code::Override {
            disabled: self.previous.as_ref().is_some_and(|value| value.disabled),
            command: (!values[0].is_empty()).then(|| values[0].clone()),
            args,
            config,
            environment,
        };
        if self.previous.as_ref() == Some(&script)
            || (self.previous.is_none() && values.iter().all(String::is_empty))
        {
            return true;
        }
        let applied = self.owner.update(cx, |panel, cx| {
            if !panel.active || panel.loading {
                panel.error = Some("code_target_changed");
                cx.notify();
                return false;
            }
            let Some(draft) = panel.draft.as_mut() else {
                return false;
            };
            if draft.servers.get(&self.server) != self.previous.as_ref() {
                panel.error = Some("code_target_changed");
                cx.notify();
                return false;
            }
            draft.servers.insert(self.server.clone(), script.clone());
            panel.save(window, cx);
            true
        });
        if applied {
            self.previous = Some(script);
        }
        applied
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
