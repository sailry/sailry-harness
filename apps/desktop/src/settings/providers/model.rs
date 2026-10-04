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
    checkbox::Checkbox,
    collapsible::Collapsible,
    form::{Field, Form},
    input::{Input, InputState},
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

const DEFAULT_CONTEXT: u32 = 200_000;
const DEFAULT_OUTPUT: u32 = 16_384;

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
        let reported_output = value.output.or(reference.and_then(|model| model.output));
        let context = value
            .context
            .or(reference.and_then(|model| model.context))
            .unwrap_or_else(|| DEFAULT_CONTEXT.max(reported_output.unwrap_or(0)));
        let output = reported_output.unwrap_or(DEFAULT_OUTPUT.min(context));
        let capabilities = value.capabilities.as_ref();
        let reasoning = capabilities
            .and_then(|value| value.reasoning)
            .or(reference.and_then(|model| model.reasoning))
            .unwrap_or(false);
        let mut efforts = capabilities
            .and_then(|value| value.efforts.clone())
            .unwrap_or_else(|| {
                reference
                    .map(|model| model.efforts(preset.api(), output))
                    .unwrap_or_default()
            });
        efforts.retain(|effort| *effort != Effort::Default);
        let default_effort = Effort::initial(
            &efforts,
            capabilities
                .and_then(|value| value.default_effort)
                .unwrap_or(Effort::Default),
        );
        let missing = completion::Missing::new(&value, reference);
        let model = Model {
            id: value.id,
            context,
            output,
            vision: capabilities
                .and_then(|value| value.vision)
                .unwrap_or_else(|| {
                    reference.is_some_and(|model| model.inputs.iter().any(|value| value == "image"))
                }),
            tools: capabilities
                .and_then(|value| value.tools)
                .unwrap_or_else(|| reference.is_some_and(|model| model.tools == Some(true))),
            reasoning,
            web_search: capabilities
                .and_then(|value| value.web_search)
                .unwrap_or(false),
            generates: reference
                .map(|model| {
                    model
                        .outputs
                        .iter()
                        .filter_map(|output| match output.as_str() {
                            "image" => Some(sailry_protocol::media::Generation::Image),
                            "video" => Some(sailry_protocol::media::Generation::Video),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            efforts,
            custom_efforts: false,
            default_effort,
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
        let mut model = Model {
            id,
            context,
            output,
            ..self.model.clone()
        };
        let efforts = self.efforts.apply(&mut model, cx);
        if model.reasoning {
            efforts?;
        }
        Ok(model)
    }
}

impl Editor {
    pub fn render_model(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let draft = &self.models[index];
        let key = draft.key;
        let title = draft.id.read(cx).value();
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
            let mut capabilities = h_flex().flex_wrap().gap_4().child(
                Checkbox::new(("model-text", key))
                    .text_sm()
                    .label(tr("provider_cap_text"))
                    .checked(true)
                    .disabled(true),
            );
            for (kind, label, checked) in [
                (0, "provider_cap_vision", draft.model.vision),
                (1, "provider_cap_tools", draft.model.tools),
                (2, "provider_cap_reasoning", draft.model.reasoning),
                (
                    4,
                    "provider_cap_image",
                    draft
                        .model
                        .generates
                        .contains(&sailry_protocol::media::Generation::Image),
                ),
                (
                    5,
                    "provider_cap_video",
                    draft
                        .model
                        .generates
                        .contains(&sailry_protocol::media::Generation::Video),
                ),
            ] {
                capabilities = capabilities.child(
                    Checkbox::new((label, key))
                        .text_sm()
                        .debug_selector(move || format!("model-capability-{key}-{kind}"))
                        .disabled(self.pending)
                        .label(tr(label))
                        .checked(checked)
                        .on_click(cx.listener(move |editor, checked, window, cx| {
                            let draft = &mut editor.models[index];
                            let model = &mut draft.model;
                            match kind {
                                0 => {
                                    draft.missing.vision = false;
                                    model.vision = *checked;
                                }
                                1 => {
                                    draft.missing.tools = false;
                                    model.tools = *checked;
                                }
                                2 => {
                                    draft.missing.reasoning = false;
                                    model.reasoning = *checked;
                                    if !checked {
                                        editor.focus.focus(window, cx);
                                    }
                                }
                                _ => {
                                    draft.missing.generates = false;
                                    let output = if kind == 4 {
                                        sailry_protocol::media::Generation::Image
                                    } else {
                                        sailry_protocol::media::Generation::Video
                                    };
                                    model.generates.retain(|value| *value != output);
                                    if *checked {
                                        model.generates.push(output);
                                    }
                                }
                            }
                            cx.notify();
                        })),
                );
            }
            let details = v_flex()
                .gap_4()
                .pt_3()
                .child(capabilities)
                .when(draft.model.reasoning, |details| {
                    details.child(self.reasoning(index, cx))
                });
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
            card = card.child(form).child(self.limits(index, cx)).child(
                Collapsible::new()
                    .open(draft.advanced)
                    .child(h_flex().child(advanced))
                    .content(details),
            );
        }
        card.into_any_element()
    }
}
