use super::{data::Model, editor::Editor};
use sailry_protocol::Effort;
mod completion;
mod limits;
mod reasoning;
#[cfg(test)]
mod tests;
use crate::tr;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    collapsible::Collapsible,
    form::{Field, Form},
    input::{Input, InputState},
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

pub(super) struct Draft {
    pub key: usize,
    pub id: Entity<InputState>,
    context: Entity<InputState>,
    efforts: reasoning::Rows,
    pub expanded: bool,
    pub advanced: bool,
    model: Model,
    missing: completion::Missing,
}

impl Draft {
    pub fn discovered(
        key: usize,
        value: sailry_protocol::conversation::discovery::Model,
        reference: Option<&sailry_protocol::conversation::catalog::Model>,
        preset: super::Preset,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let missing = completion::Missing::new(&value, reference);
        let completed = value.configuration(reference, preset.api(), preset.authentication());
        let model = Model {
            id: completed.id,
            display_name: completed.display_name,
            context: completed.context,
            output: completed.output,
            capabilities: completed.capabilities,
            vision: completed.vision,
            tools: completed.tools,
            reasoning: completed.reasoning,
            web_search: completed.web_search,
            generates: completed.generates,
            efforts: completed.efforts,
            custom_efforts: completed.custom_efforts,
            default_effort: completed.default_effort,
        };
        let mut draft = Self::new(key, model, window, cx);
        draft.missing = missing;
        draft
    }

    pub fn new(key: usize, mut model: Model, window: &mut Window, cx: &mut App) -> Self {
        model.efforts.retain(|effort| *effort != Effort::Default);
        model.default_effort = Effort::initial(&model.efforts, model.default_effort);
        Self {
            key,
            id: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(model.id.clone())
                    .placeholder(tr("form_model_hint"))
            }),
            context: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(model.context.to_string())
                    .placeholder(tr("form_tokens_hint"))
            }),
            efforts: reasoning::Rows::new(&model, window, cx),
            expanded: false,
            advanced: false,
            model,
            missing: Default::default(),
        }
    }

    pub fn value(&self, cx: &App) -> Result<Model, &'static str> {
        let id = self.id.read(cx).value().trim().to_owned();
        if id.is_empty() {
            return Err("provider_model_required");
        }
        let context = self
            .context
            .read(cx)
            .value()
            .trim()
            .parse::<u32>()
            .unwrap_or(0);
        let output = self.model.output;
        if context == 0 || output == 0 || output > context {
            return Err("provider_limits_invalid");
        }
        let unchanged = id == self.model.id;
        let mut model = Model {
            display_name: self.model.display_name.clone().filter(|_| unchanged),
            id,
            context,
            output,
            capabilities: self.model.capabilities.clone().filter(|_| unchanged),
            ..self.model.clone()
        };
        let efforts = self.efforts.apply(&mut model, cx);
        if model
            .capabilities
            .as_ref()
            .and_then(|value| value.reasoning)
            != Some(false)
        {
            efforts?;
        }
        Ok(model)
    }
}

impl Editor {
    pub fn render_model(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let draft = &self.models[index];
        let key = draft.key;
        let id = draft.id.read(cx).value();
        let unchanged = id.as_ref() == draft.model.id;
        let title = if unchanged {
            SharedString::from(draft.model.label())
        } else {
            id
        };
        let title = if title.is_empty() {
            tr("provider_new_model")
        } else {
            title
        };
        let selected = self.default_model == index;
        let header = h_flex()
            .gap_2()
            .child(
                Button::new(("model-expand", key))
                    .debug_selector(move || format!("model-expand-{key}"))
                    .text()
                    .flex_1()
                    .min_w_0()
                    .h_8()
                    .accessibility_label(title.clone())
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .child(
                                Icon::new(if draft.expanded {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .small(),
                            )
                            .child(
                                div()
                                    .debug_selector(move || format!("model-title-{key}"))
                                    .min_w_0()
                                    .text_ellipsis()
                                    .child(title),
                            ),
                    )
                    .on_click(cx.listener(move |editor, _, window, cx| {
                        editor.models[index].expanded = !editor.models[index].expanded;
                        if !editor.models[index].expanded {
                            editor.focus.focus(window, cx);
                        }
                        cx.notify();
                    })),
            )
            .child(
                Button::new(("model-default", key))
                    .debug_selector(move || format!("model-default-{key}"))
                    .ghost()
                    .small()
                    .icon(
                        Icon::new(IconName::Check)
                            .when(selected, |icon| icon.text_color(cx.theme().success)),
                    )
                    .tooltip(tr(if selected {
                        "provider_default"
                    } else {
                        "provider_set_default"
                    }))
                    .accessibility_label(tr(if selected {
                        "provider_default"
                    } else {
                        "provider_set_default"
                    }))
                    .disabled(selected || self.pending)
                    .on_click(cx.listener(move |editor, _, _, cx| {
                        editor.default_model = index;
                        cx.notify();
                    })),
            )
            .child(
                Button::new(("model-delete", key))
                    .debug_selector(move || format!("model-delete-{key}"))
                    .disabled(self.pending)
                    .ghost()
                    .small()
                    .icon(IconName::CircleX)
                    .tooltip(tr("settings_delete"))
                    .accessibility_label(tr("settings_delete"))
                    .on_click(cx.listener(move |editor, _, window, cx| {
                        editor.models.remove(index);
                        editor.focus.focus(window, cx);
                        if editor.default_model == index {
                            editor.default_model = 0;
                        } else if editor.default_model > index {
                            editor.default_model -= 1;
                        }
                        cx.notify();
                    })),
            );
        let mut card = v_flex()
            .debug_selector(move || format!("model-card-{key}"))
            .gap_3()
            .p_3()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius)
            .child(header);
        if draft.expanded {
            let form = Form::vertical().child(
                Field::new()
                    .label(tr(if self.preset == super::Preset::AzureOpenAi {
                        "provider_deployment_id"
                    } else {
                        "provider_model_id"
                    }))
                    .child(Input::new(&draft.id).disabled(self.pending).aria_label(tr(
                        if self.preset == super::Preset::AzureOpenAi {
                            "provider_deployment_id"
                        } else {
                            "provider_model_id"
                        },
                    ))),
            );
            card = card.child(form).child(self.limits(index, cx));
            if !unchanged
                || draft
                    .model
                    .capabilities
                    .as_ref()
                    .and_then(|value| value.reasoning)
                    != Some(false)
            {
                let details = v_flex().gap_4().pt_3().child(self.reasoning(index, cx));
                let advanced = Button::new(("model-advanced", key))
                    .debug_selector(move || format!("model-advanced-{key}"))
                    .text()
                    .label(tr("provider_advanced"))
                    .icon(if draft.advanced {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .on_click(cx.listener(move |editor, _, window, cx| {
                        editor.models[index].advanced = !editor.models[index].advanced;
                        if !editor.models[index].advanced {
                            editor.focus.focus(window, cx);
                        }
                        cx.notify();
                    }));
                card = card.child(
                    Collapsible::new()
                        .open(draft.advanced)
                        .child(h_flex().child(advanced))
                        .content(details),
                );
            }
        }
        card.into_any_element()
    }
}
