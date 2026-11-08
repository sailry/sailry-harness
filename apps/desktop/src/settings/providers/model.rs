use super::{data::Model, editor::Editor};
use sailry_protocol::Effort;
mod completion;
mod limits;
mod reasoning;
use crate::tr;
use crate::ui::modal_list::Row;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    form::Field,
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
            compaction_tokens: completed.compaction_tokens,
            capabilities: completed.capabilities,
            vision: completed.vision,
            tools: completed.tools,
            reasoning: completed.reasoning,
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
    /// One row of the editor's model list; the per-model form expands under it.
    pub fn render_model(&self, index: usize, cx: &mut Context<Self>) -> Row {
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
        let default = Button::new(("model-default", key))
            .debug_selector(move || format!("model-default-{key}"))
            .ghost()
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
            }));
        let content = draft
            .expanded
            .then(|| self.model_form(index, unchanged, cx));
        Row::new(format!("model-{key}"), title)
            .when(selected, |row| row.meta(tr("provider_model_default")))
            .trailing(default)
            .remove(
                tr("settings_delete"),
                cx.listener(move |editor, _, window, cx| {
                    if editor.pending {
                        return;
                    }
                    editor.models.remove(index);
                    editor.focus.focus(window, cx);
                    if editor.default_model == index {
                        editor.default_model = 0;
                    } else if editor.default_model > index {
                        editor.default_model -= 1;
                    }
                    cx.notify();
                }),
            )
            .expanded(draft.expanded, div().children(content))
            .on_click(cx.listener(move |editor, _, window, cx| {
                editor.models[index].expanded = !editor.models[index].expanded;
                if !editor.models[index].expanded {
                    editor.focus.focus(window, cx);
                }
                cx.notify();
            }))
    }

    fn model_form(&self, index: usize, unchanged: bool, cx: &mut Context<Self>) -> AnyElement {
        let draft = &self.models[index];
        let key = draft.key;
        let form = crate::ui::form::vertical().child(
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
        let mut content = v_flex()
            .debug_selector(move || format!("model-card-{key}"))
            .w_full()
            .gap_3()
            .child(form)
            .child(self.limits(index, cx));
        if !unchanged
            || draft
                .model
                .capabilities
                .as_ref()
                .and_then(|value| value.reasoning)
                != Some(false)
        {
            content = content.child(self.reasoning(index, cx));
        }
        content.into_any_element()
    }
}
