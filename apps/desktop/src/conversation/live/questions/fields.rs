//! Flat MCP forms use Kit inputs and retain only local drafts.
use super::*;
use sailry_protocol::conversation::question::form::{Field as Spec, Input as Type};
use serde_json::{Map, Value};

pub(super) struct Draft {
    pub input: crate::conversation::interaction::Draft,
    pub included: bool,
    touched: bool,
}

impl Draft {
    pub fn new(field: &Spec, window: &mut Window, cx: &mut Context<View>) -> Self {
        let kind = match &field.input {
            Type::Text { format, .. } => Kind::Text {
                multiline: format.is_none(),
                max_bytes: 64 * 1024,
            },
            Type::Number { .. } => Kind::Text {
                multiline: false,
                max_bytes: 64 * 1024,
            },
            Type::Boolean => Kind::Choice {
                options: vec![tr("question_yes"), tr("question_no")],
                multiple: false,
                allow_other: false,
            },
            Type::Choice {
                options, multiple, ..
            } => Kind::Choice {
                options: options
                    .iter()
                    .map(|option| option.title.clone().into())
                    .collect(),
                multiple: *multiple,
                allow_other: false,
            },
        };
        Self {
            input: crate::conversation::interaction::Draft::new(&kind, window, cx),
            included: field.required,
            touched: false,
        }
    }

    pub fn empty(&self, cx: &App) -> bool {
        !self.touched
            && self.input.selected.is_empty()
            && self
                .input
                .field
                .as_ref()
                .is_none_or(|field| field.value(cx).is_empty())
    }
}

pub(super) fn display(field: &Spec, value: &Value) -> String {
    match (&field.input, value) {
        (Type::Boolean, Value::Bool(value)) => tr(if *value {
            "question_yes"
        } else {
            "question_no"
        })
        .to_string(),
        (Type::Choice { options, .. }, Value::String(value)) => options
            .iter()
            .find(|option| &option.value == value)
            .map(|option| option.title.clone())
            .unwrap_or_else(|| value.clone()),
        (Type::Choice { options, .. }, Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(|value| {
                options
                    .iter()
                    .find(|option| option.value == value)
                    .map(|option| option.title.as_str())
                    .unwrap_or(value)
            })
            .collect::<Vec<_>>()
            .join(", "),
        (_, Value::String(value)) => value.clone(),
        _ => value.to_string(),
    }
}

pub(super) fn answer(fields: &[Spec], drafts: &[Draft], cx: &App) -> Option<Map<String, Value>> {
    let mut values = Map::new();
    for (field, draft) in fields.iter().zip(drafts) {
        if !draft.included {
            continue;
        }
        let text = || {
            draft
                .input
                .field
                .as_ref()
                .map(|input| input.value(cx))
                .unwrap_or_default()
        };
        let value = match &field.input {
            Type::Text { .. } => Value::String(text()),
            Type::Number { .. } => Value::Number(text().trim().parse().ok()?),
            Type::Boolean => Value::Bool(*draft.input.selected.first()? == 0),
            Type::Choice {
                options,
                multiple: false,
                ..
            } => Value::String(options.get(*draft.input.selected.first()?)?.value.clone()),
            Type::Choice {
                options,
                multiple: true,
                ..
            } => Value::Array(
                draft
                    .input
                    .selected
                    .iter()
                    .map(|index| {
                        options
                            .get(*index)
                            .map(|option| Value::String(option.value.clone()))
                    })
                    .collect::<Option<_>>()?,
            ),
        };
        values.insert(field.name.clone(), value);
    }
    Some(values)
}

impl View {
    fn change_field(
        &mut self,
        id: QuestionId,
        index: usize,
        change: impl FnOnce(&Spec, &mut Draft),
        cx: &mut Context<Self>,
    ) {
        if self.questions.locked() || self.question_pending(id).is_none() {
            return;
        }
        let Some(editing) = self
            .questions
            .editing
            .as_mut()
            .filter(|editing| editing.id == id)
        else {
            return;
        };
        let Input::Form { fields } = &editing.spec.input else {
            return;
        };
        if let Some((field, draft)) = fields.get(index).zip(editing.fields.get_mut(index)) {
            change(field, draft);
            draft.touched = true;
            cx.notify();
        }
    }

    pub(super) fn form_fields(
        &self,
        editing: &Editing,
        editable: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use gpui_kit::component::{
            checkbox::Checkbox,
            input::{Input as TextInput, Textarea},
            radio::{Radio, RadioGroup},
        };
        use gpui_kit::prelude::FluentBuilder as _;
        let Input::Form { fields } = &editing.spec.input else {
            return div().into_any_element();
        };
        let id = editing.id;
        v_flex()
            .w_full()
            .gap_4()
            .children(fields.iter().zip(&editing.fields).enumerate().map(
                |(index, (field, draft))| {
                    let enabled = editable && draft.included;
                    let mut row = v_flex()
                        .w_full()
                        .gap_2()
                        .child(if field.required {
                            div()
                                .text_sm()
                                .font_medium()
                                .child(format!("{} *", field.title))
                                .into_any_element()
                        } else {
                            Checkbox::new(format!("question-{id}-include-{index}"))
                                .debug_selector(move || format!("live-form-include-{index}"))
                                .small()
                                .w_full()
                                .label(
                                    rust_i18n::t!("question_optional", field = field.title)
                                        .to_string(),
                                )
                                .checked(draft.included)
                                .disabled(!editable)
                                .on_click(cx.listener(move |view, included, _, cx| {
                                    view.change_field(
                                        id,
                                        index,
                                        |_, draft| draft.included = *included,
                                        cx,
                                    )
                                }))
                                .into_any_element()
                        })
                        .when_some(
                            field.description.as_ref().filter(|text| !text.is_empty()),
                            |row, description| {
                                row.child(
                                    div()
                                        .text_xs()
                                        .whitespace_normal()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(description.clone()),
                                )
                            },
                        );
                    if let Some(input) = &draft.input.field {
                        row = row.child(
                            div()
                                .w_full()
                                .debug_selector(move || format!("live-form-input-{index}"))
                                .child(match input {
                                    Field::Line(input) => TextInput::new(input)
                                        .aria_label(field.title.clone())
                                        .readonly(!enabled)
                                        .into_any_element(),
                                    Field::Multiline(input) => Textarea::new(input)
                                        .aria_label(field.title.clone())
                                        .readonly(!enabled)
                                        .into_any_element(),
                                }),
                        );
                    } else {
                        let (labels, multiple): (Vec<SharedString>, bool) = match &field.input {
                            Type::Boolean => (vec![tr("question_yes"), tr("question_no")], false),
                            Type::Choice {
                                options, multiple, ..
                            } => (
                                options
                                    .iter()
                                    .map(|option| option.title.clone().into())
                                    .collect(),
                                *multiple,
                            ),
                            _ => unreachable!("text fields own an input"),
                        };
                        if multiple {
                            row = row.children(labels.into_iter().enumerate().map(
                                |(choice, label)| {
                                    Checkbox::new(format!(
                                        "question-{id}-field-{index}-choice-{choice}"
                                    ))
                                    .debug_selector(move || {
                                        format!("live-form-choice-{index}-{choice}")
                                    })
                                    .small()
                                    .w_full()
                                    .label(label)
                                    .checked(draft.input.selected.contains(&choice))
                                    .disabled(!enabled)
                                    .on_click(cx.listener(
                                        move |view, selected, _, cx| {
                                            view.change_field(
                                                id,
                                                index,
                                                |_, draft| {
                                                    if *selected {
                                                        draft.input.selected.insert(choice);
                                                    } else {
                                                        draft.input.selected.remove(&choice);
                                                    }
                                                },
                                                cx,
                                            )
                                        },
                                    ))
                                },
                            ));
                        } else {
                            row = row.child(
                                RadioGroup::vertical(format!("question-{id}-field-{index}"))
                                    .w_full()
                                    .gap_3()
                                    .selected_index(draft.input.selected.first().copied())
                                    .disabled(!enabled)
                                    .children(labels.into_iter().enumerate().map(
                                        |(choice, label)| {
                                            Radio::new(choice)
                                                .small()
                                                .w_full()
                                                .label(label)
                                                .debug_selector(move || {
                                                    format!("live-form-choice-{index}-{choice}")
                                                })
                                        },
                                    ))
                                    .on_click(cx.listener(move |view, choice, _, cx| {
                                        view.change_field(
                                            id,
                                            index,
                                            |_, draft| {
                                                draft.input.selected.clear();
                                                draft.input.selected.insert(*choice);
                                            },
                                            cx,
                                        )
                                    })),
                            );
                        }
                    }
                    row
                },
            ))
            .into_any_element()
    }
}
