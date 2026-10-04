use super::*;
use gpui_kit::component::{
    form::Field,
    input::{Input, NumberInput},
    select::Select,
    switch::Switch,
};

impl Setting {
    pub(super) fn label(&self) -> String {
        self.spec
            .title
            .clone()
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| self.name.clone())
    }

    pub fn field(entity: &Entity<Self>, cx: &App) -> Field {
        let setting = entity.read(cx);
        if setting.model() {
            return Field::new().label_indent(false).child(entity.clone());
        }
        let mut field = Field::new()
            .label(setting.label())
            .required(setting.required && !setting.panel)
            .child(entity.clone());
        if let Some(description) = setting
            .spec
            .description
            .clone()
            .filter(|text| !text.is_empty())
        {
            field = field.description(description);
        }
        field
    }
}

impl Render for Setting {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let label = self.label();
        let control: AnyElement = match &self.control {
            Control::Text(input) => Input::new(input)
                .disabled(self.locked)
                .aria_label(label.clone())
                .into_any_element(),
            Control::Number(input) => NumberInput::new(input)
                .disabled(self.locked)
                .into_any_element(),
            Control::Bool(value) => h_flex()
                .gap_2()
                .child(
                    div()
                        .debug_selector({
                            let name = self.name.clone();
                            move || format!("plugin-boolean-{name}")
                        })
                        .child(
                            Switch::new("boolean")
                                .checked(*value)
                                .disabled(self.locked)
                                .accessibility_label(label.clone())
                                .on_click(cx.listener(|field, value, _, cx| {
                                    if let Control::Bool(current) = &mut field.control {
                                        *current = *value;
                                    }
                                    field.edited(cx);
                                })),
                        ),
                )
                .when(!self.present, |view| {
                    view.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr("plugins_settings_unset")),
                    )
                })
                .into_any_element(),
            Control::Choice(choice) => Select::new(choice)
                .disabled(self.locked)
                .cleanable(!self.panel && !self.required)
                .placeholder(tr("plugins_settings_choose"))
                .accessibility_label(label.clone())
                .w_full()
                .into_any_element(),
            Control::Model { .. } => return self.model_fields(cx),
            Control::Secret {
                input,
                intent,
                configured,
            } => {
                let action = intent.read(cx).selected_value().and_then(Json::as_str);
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .debug_selector({
                                let name = self.name.clone();
                                move || format!("plugin-intent-{name}")
                            })
                            .child(
                                Select::new(intent)
                                    .disabled(self.locked)
                                    .placeholder(tr("plugins_settings_choose"))
                                    .accessibility_label(label.clone())
                                    .w_full(),
                            ),
                    )
                    .when(action == Some("replace"), |view| {
                        view.child(
                            div()
                                .debug_selector({
                                    let name = self.name.clone();
                                    move || format!("plugin-secret-{name}")
                                })
                                .child(
                                    Input::new(input)
                                        .disabled(self.locked)
                                        .aria_label(label.clone()),
                                ),
                        )
                    })
                    .when(action == Some("keep"), |view| {
                        view.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(tr(if *configured {
                                    "plugins_settings_configured"
                                } else {
                                    "plugins_settings_unset"
                                })),
                        )
                    })
                    .when(action == Some("clear"), |view| {
                        view.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().danger)
                                .child(tr("plugins_settings_clear_effect")),
                        )
                    })
                    .into_any_element()
            }
        };
        self.input_row(control, cx).into_any_element()
    }
}

impl Setting {
    pub(super) fn input_row(
        &self,
        control: AnyElement,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = self.label();
        let selector = format!("plugin-setting-{}", self.name);
        let optional = !self.panel
            && !self.required
            && self.spec.secret.is_none()
            && !matches!(self.control, Control::Choice(_));
        h_flex()
            .id("field")
            .role(gpui::Role::Group)
            .aria_label(label)
            .gap_2()
            .items_start()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .debug_selector(move || selector.clone())
                    .child(control),
            )
            .when(optional, |view| {
                view.child(
                    Button::new("unset")
                        .debug_selector({
                            let name = self.name.clone();
                            move || format!("plugin-unset-{name}")
                        })
                        .ghost()
                        .icon(IconName::CircleX)
                        .disabled(self.locked || !self.present)
                        .tooltip(tr("plugins_settings_unset_action"))
                        .accessibility_label(tr("plugins_settings_unset_action"))
                        .on_click(cx.listener(|field, _, window, cx| field.unset(window, cx))),
                )
            })
    }
}
