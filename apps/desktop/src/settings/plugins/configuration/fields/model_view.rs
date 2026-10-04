use super::*;
use gpui_kit::component::{form::Field as FormField, popover::Popover};

impl Setting {
    pub(super) fn model_fields(&self, cx: &mut Context<Self>) -> AnyElement {
        let Control::Model {
            picker,
            selected,
            catalog,
            ..
        } = &self.control
        else {
            unreachable!("model fields require a model control")
        };
        let label = self.label();
        let title = catalog
            .models
            .iter()
            .find(|model| Some(&model.id) == selected.as_ref())
            .map(|model| model.model.clone().into())
            .or_else(|| selected.clone().map(SharedString::from))
            .unwrap_or_else(|| tr("plugins_settings_choose"));
        let picker = picker.clone();
        picker.update(cx, |picker, cx| {
            picker.catalog(catalog, selected.as_deref(), cx)
        });
        let control = div()
            .debug_selector({
                let name = self.name.clone();
                move || format!("plugin-model-choice-{name}")
            })
            .child(
                Popover::new("model-picker")
                    .p_2()
                    .anchor(Anchor::TopLeft)
                    .trigger(
                        Button::new("model")
                            .outline()
                            .w_full()
                            .disabled(self.locked)
                            .accessibility_label(label.clone())
                            .child(
                                h_flex()
                                    .text_sm()
                                    .w_full()
                                    .gap_2()
                                    .child(div().flex_1().min_w_0().truncate().child(title))
                                    .child(IconName::ChevronDown),
                            ),
                    )
                    .content(move |_, _, cx| {
                        let popover = cx.entity().downgrade();
                        picker.update(cx, |picker, _| picker.popover = Some(popover));
                        picker.clone()
                    }),
            );
        let mut model = FormField::new()
            .label(label)
            .required(self.required && !self.panel)
            .child(self.input_row(control.into_any_element(), cx));
        if let Some(description) = self
            .spec
            .description
            .clone()
            .filter(|text| !text.is_empty())
        {
            model = model.description(description);
        }
        gpui_kit::component::form::Form::vertical()
            .gap_4()
            .child(model)
            .into_any_element()
    }
}
