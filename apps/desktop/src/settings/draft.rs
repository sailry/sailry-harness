use crate::theme::DialogStyle as _;
use crate::tr;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    form::Field,
    input::{Input, InputState},
    scroll::ScrollableElement,
    *,
};
use gpui_kit::*;

type Save = Box<dyn Fn(&[String], &mut Window, &mut App) -> Result<(), &'static str>>;

struct Editor {
    fields: Vec<(&'static str, Entity<InputState>)>,
    save: Save,
    error: Option<&'static str>,
}

// Shared only by the text-record settings dialogs (environment and Git skills).
pub fn open(
    title: &'static str,
    fields: Vec<(&'static str, String)>,
    save: impl Fn(&[String], &mut Window, &mut App) -> Result<(), &'static str> + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let editor = cx.new(|cx| Editor {
        fields: fields
            .into_iter()
            .map(|(key, value)| {
                (
                    key,
                    cx.new(|cx| {
                        InputState::new(window, cx)
                            .default_value(value)
                            .placeholder(tr(match key {
                                "shortcut_binding" => "shortcut_binding_hint",
                                "terminal_variable" => "form_variable_hint",
                                "terminal_value" => "form_value_hint",
                                "skills_url" => "project_repository_placeholder",
                                "skills_ref" => "form_revision_hint",
                                "skills_subdirectory" => "form_subdirectory_hint",
                                "skills_namespace" => "form_namespace_hint",
                                _ => "form_name_hint",
                            }))
                    }),
                )
            })
            .collect(),
        save: Box::new(save),
        error: None,
    });
    editor.update(cx, |_, cx| {
        crate::feedback::observe(window, cx, |view, _| view.error.into_iter().collect());
    });
    window.open_dialog(cx, move |dialog, window, _| {
        let save = editor.clone();
        dialog
            .form_title(tr(title))
            .w((window.viewport_size().width - px(48.)).min(px(560.)))
            .child(editor.clone())
            .footer(
                gpui_kit::component::dialog::DialogFooter::new()
                    .w_full()
                    .gap_2()
                    .child(
                        Button::new("record-cancel")
                            .label(tr("settings_cancel"))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("record-save")
                            .primary()
                            .debug_selector(|| "record-save".into())
                            .label(tr("settings_save"))
                            .on_click(move |_, window, cx| {
                                save.update(cx, |editor, cx| {
                                    let values: Vec<_> = editor
                                        .fields
                                        .iter()
                                        .map(|(_, input)| input.read(cx).value().to_string())
                                        .collect();
                                    match (editor.save)(&values, window, cx) {
                                        Ok(()) => window.close_dialog(cx),
                                        Err(error) => {
                                            editor.error = Some(error);
                                            cx.notify();
                                        }
                                    }
                                })
                            }),
                    ),
            )
    });
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("record-editor")
            .max_h((window.viewport_size().height - px(230.)).max(px(160.)))
            .overflow_y_scrollbar()
            .gap_4()
            .child(
                gpui_kit::component::form::Form::vertical().children(self.fields.iter().map(
                    |(key, state)| {
                        Field::new()
                            .label(tr(key))
                            .child(Input::new(state).aria_label(tr(key)))
                    },
                )),
            )
    }
}

pub fn confirm(
    title: &'static str,
    action: impl Fn(&mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    crate::prompts::confirm(
        &tr(title),
        &tr("settings_remove_preview"),
        tr("settings_delete"),
        window,
        cx,
        move |_, cx| action(cx),
    );
}
