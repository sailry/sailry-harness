use super::*;

struct Row {
    key: usize,
    input: Entity<InputState>,
}

pub(super) struct Rows {
    entries: Vec<Row>,
    default: usize,
    next: usize,
    edited: bool,
}

#[derive(Clone, Copy)]
enum Edit {
    Add,
    Remove(usize),
    Default(usize),
}

fn parse(value: &str) -> Result<Effort, &'static str> {
    let effort = match value.trim() {
        "none" => Effort::Disabled,
        "minimal" => Effort::Minimal,
        "low" => Effort::Low,
        "medium" => Effort::Medium,
        "high" => Effort::High,
        "xhigh" => Effort::XHigh,
        "max" => Effort::Max,
        "dynamic" => Effort::Budget(-1),
        tokens => Effort::Budget(tokens.parse().map_err(|_| "provider_effort_invalid")?),
    };
    Ok(effort)
}

impl Rows {
    pub(super) fn new(model: &Model, window: &mut Window, cx: &mut App) -> Self {
        let entries = model
            .efforts
            .iter()
            .enumerate()
            .map(|(key, &effort)| {
                let value = match effort {
                    Effort::Budget(tokens) => tokens.to_string(),
                    _ => crate::reasoning::label(effort).to_string(),
                };
                Row {
                    key,
                    input: cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder(tr("provider_effort_placeholder"))
                            .default_value(value)
                    }),
                }
            })
            .collect();
        Self {
            entries,
            default: model
                .efforts
                .iter()
                .position(|effort| *effort == model.default_effort)
                .unwrap_or(0),
            next: model.efforts.len(),
            edited: false,
        }
    }

    pub(super) fn apply(&self, model: &mut Model, cx: &App) -> Result<(), &'static str> {
        let efforts = self
            .entries
            .iter()
            .map(|row| parse(&row.input.read(cx).value()))
            .collect::<Result<Vec<_>, _>>()?;
        let default = self
            .entries
            .iter()
            .position(|row| row.key == self.default)
            .map(|index| efforts[index])
            .unwrap_or(Effort::Default);
        model.custom_efforts |=
            self.edited || efforts != model.efforts || default != model.default_effort;
        model.efforts = efforts;
        model.default_effort = default;
        Ok(())
    }

    fn edit(&mut self, edit: Edit, window: &mut Window, cx: &mut App) {
        match edit {
            Edit::Add if self.entries.len() < 16 => {
                let input = cx.new(|cx| {
                    InputState::new(window, cx).placeholder(tr("provider_effort_placeholder"))
                });
                input.update(cx, |input, cx| input.focus(window, cx));
                if self.entries.is_empty() {
                    self.default = self.next;
                }
                self.entries.push(Row {
                    key: self.next,
                    input,
                });
                self.next += 1;
            }
            Edit::Remove(key) => {
                self.entries.retain(|row| row.key != key);
                if self.default == key {
                    self.default = self.entries.first().map_or(self.next, |row| row.key);
                }
            }
            Edit::Default(key) if self.entries.iter().any(|row| row.key == key) => {
                self.default = key;
            }
            _ => return,
        }
        self.edited = true;
    }
}

impl Editor {
    fn change_effort(
        &mut self,
        key: usize,
        edit: Edit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending {
            return;
        }
        let Some(draft) = self.models.iter_mut().find(|draft| draft.key == key) else {
            return;
        };
        draft.efforts.edit(edit, window, cx);
        if matches!(edit, Edit::Remove(_)) {
            self.focus.focus(window, cx);
        }
        self.error = None;
        cx.notify();
    }

    pub(super) fn reasoning(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let draft = &self.models[index];
        let key = draft.key;
        let rows = &draft.efforts;
        let add = Button::new(("effort-add", key))
            .debug_selector(move || format!("effort-add-{key}"))
            .label(tr("settings_add"))
            .disabled(self.pending || rows.entries.len() >= 16)
            .on_click(cx.listener(move |editor, _, window, cx| {
                editor.change_effort(key, Edit::Add, window, cx)
            }));
        let mut view = v_flex().id(("model-efforts", key)).gap_2().child(
            h_flex()
                .gap_3()
                .justify_between()
                .child(div().text_sm().child(tr("provider_efforts")))
                .child(add),
        );
        for row in &rows.entries {
            let row_key = row.key;
            let selected = row_key == rows.default;
            let default = Button::new(("effort-default", row_key))
                .debug_selector(move || format!("effort-default-{key}-{row_key}"))
                .ghost()
                .small()
                .icon(
                    Icon::new(IconName::Check)
                        .when(selected, |icon| icon.text_color(cx.theme().success)),
                )
                .tooltip(tr(if selected {
                    "provider_effort_default"
                } else {
                    "provider_set_default"
                }))
                .accessibility_label(tr(if selected {
                    "provider_effort_default"
                } else {
                    "provider_set_default"
                }))
                .disabled(self.pending || selected)
                .on_click(cx.listener(move |editor, _, window, cx| {
                    editor.change_effort(key, Edit::Default(row_key), window, cx)
                }));
            let remove = Button::new(("effort-remove", row_key))
                .debug_selector(move || format!("effort-remove-{key}-{row_key}"))
                .ghost()
                .small()
                .icon(IconName::CircleX)
                .tooltip(tr("settings_delete"))
                .accessibility_label(tr("settings_delete"))
                .disabled(self.pending)
                .on_click(cx.listener(move |editor, _, window, cx| {
                    editor.change_effort(key, Edit::Remove(row_key), window, cx)
                }));
            view = view.child(
                h_flex()
                    .id(("effort-row", row_key))
                    .debug_selector(move || format!("effort-row-{key}-{row_key}"))
                    .gap_2()
                    .child(
                        div()
                            .debug_selector(move || format!("effort-input-{key}-{row_key}"))
                            .flex_1()
                            .min_w_0()
                            .child(
                                Input::new(&row.input)
                                    .disabled(self.pending)
                                    .aria_label(tr("provider_efforts")),
                            ),
                    )
                    .child(default)
                    .child(remove),
            );
        }
        view.into_any_element()
    }
}

#[cfg(test)]
mod tests;
