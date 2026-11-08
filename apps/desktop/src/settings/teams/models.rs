//! Role and lead model selection: a "follow" choice above the shared Kit model picker,
//! plus the selected model's reasoning effort. None follows the lead or the conversation.
use super::draft;
use crate::{
    model_picker::{Picked, Picker, Selection},
    settings::{Channel, Model},
    tr,
};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
    popover::Popover,
    radio::Radio,
    separator::Separator,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sailry_protocol::{Effort, ProviderId, conversation::features::Feature, role};

#[cfg(test)]
mod tests;

/// Enabled provider channels and their Node provider identities.
#[derive(Clone, Default)]
pub(super) struct Models {
    channels: Vec<Channel>,
    providers: Vec<(usize, ProviderId)>,
    binding: Option<crate::model_picker::Binding>,
}

impl Models {
    pub fn new(
        channels: Vec<Channel>,
        providers: impl IntoIterator<Item = (usize, ProviderId)>,
        binding: crate::model_picker::Binding,
    ) -> Self {
        Self {
            channels,
            providers: providers.into_iter().collect(),
            binding: Some(binding),
        }
    }

    fn channel_key(&self, provider: ProviderId) -> Option<usize> {
        self.providers
            .iter()
            .find(|(_, id)| *id == provider)
            .map(|(key, _)| *key)
    }

    fn provider(&self, channel: usize) -> Option<ProviderId> {
        self.providers
            .iter()
            .find(|(key, _)| *key == channel)
            .map(|(_, id)| *id)
    }

    pub fn find(&self, model: &role::Model) -> Option<&Model> {
        let key = self.channel_key(model.provider)?;
        self.channels
            .iter()
            .find(|channel| channel.id == key)?
            .models
            .iter()
            .find(|entry| entry.id == model.model)
    }

    fn selection(&self, model: &role::Model) -> Option<Selection> {
        Some(Selection {
            channel: self.channel_key(model.provider)?,
            model: model.model.clone(),
        })
    }

    /// The model's display name, or `follow` when the record has no fixed model.
    pub fn summary(&self, model: Option<&role::Model>, follow: &'static str) -> SharedString {
        match model {
            Some(model) => self
                .find(model)
                .map_or_else(|| model.model.clone(), |entry| entry.label().to_owned())
                .into(),
            None => tr(follow),
        }
    }
}

pub(super) struct ModelField {
    models: Models,
    value: Option<role::Model>,
    follow: &'static str,
    picker: Entity<Picker>,
    disabled: bool,
    _picked: Subscription,
}

impl ModelField {
    pub fn new(
        models: Models,
        value: Option<role::Model>,
        follow: &'static str,
        cx: &mut Context<Self>,
    ) -> Self {
        let selection = value.as_ref().and_then(|value| models.selection(value));
        let channels = models.channels.clone();
        let picker = cx.new(|cx| {
            let mut picker = Picker::new();
            picker.set(channels, selection, cx);
            picker.set_effort(value.as_ref().and_then(|value| value.effort), cx);
            if let Some(binding) = &models.binding {
                picker.bind_scores(vec![binding.clone()], cx);
            }
            picker
        });
        let picked = cx.subscribe(&picker, |field: &mut Self, _, event: &Picked, cx| {
            field.pick(event, cx)
        });
        Self {
            models,
            value,
            follow,
            picker,
            disabled: false,
            _picked: picked,
        }
    }

    pub fn value(&self) -> Option<&role::Model> {
        self.value.as_ref()
    }

    pub fn set(&mut self, value: Option<role::Model>, cx: &mut Context<Self>) {
        self.value = value;
        self.sync(cx);
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            cx.notify();
        }
    }

    fn pick(&mut self, event: &Picked, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let Some(provider) = self.models.provider(event.selection.channel) else {
            return;
        };
        let Some(channel) = self
            .models
            .channels
            .iter()
            .find(|channel| channel.id == event.selection.channel)
        else {
            return;
        };
        let mut value = role::Model {
            provider,
            model: event.selection.model.clone(),
            effort: None,
            features: None,
        };
        let Some(model) = self.models.find(&value) else {
            return;
        };
        value.effort = draft::effort(model, Some(event.effort));
        value.features = self.value.as_ref().and_then(|value| value.features.clone());
        if let Some(features) = &mut value.features {
            crate::settings::providers::configured_model(model)
                .retain_features(channel.preset.api(), features);
        }
        self.set(Some(value), cx);
    }

    fn effort(&mut self, effort: Option<Effort>, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if let Some(value) = &mut self.value {
            value.effort = effort;
        }
        self.sync(cx);
    }

    /// None follows the lead for every feature; otherwise sets one feature's choice.
    fn feature(&mut self, id: &str, choice: Option<Option<String>>, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if let Some(value) = &mut self.value {
            value.features = choice.map(|choice| {
                let mut features = value.features.clone().unwrap_or_default();
                match choice {
                    Some(option) => features.insert(id.to_owned(), option),
                    None => features.remove(id),
                };
                features
            });
            cx.notify();
        }
    }

    fn sync(&mut self, cx: &mut Context<Self>) {
        let selection = self
            .value
            .as_ref()
            .and_then(|value| self.models.selection(value));
        let channels = self.models.channels.clone();
        self.picker.update(cx, |picker, cx| {
            picker.set(channels, selection, cx);
            picker.set_effort(self.value.as_ref().and_then(|value| value.effort), cx);
        });
        cx.notify();
    }
}

/// Auto follows the lead; Off and each option are explicit.
fn feature_label(feature: &Feature, choice: Option<Option<&str>>) -> SharedString {
    match choice {
        None => tr("role_effort_inherit"),
        Some(None) => tr("composer_feature_off"),
        Some(Some(id)) if feature.id == "speed" && id == "fast" => tr("composer_feature_fast"),
        Some(Some(id)) => feature
            .options
            .iter()
            .find(|option| option.id == id)
            .and_then(|option| option.name.clone())
            .unwrap_or_else(|| id.to_owned())
            .into(),
    }
}

fn effort_label(effort: Option<Effort>) -> SharedString {
    effort.map_or_else(|| tr("role_effort_inherit"), crate::reasoning::label)
}

impl Render for ModelField {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.models.summary(self.value.as_ref(), self.follow);
        let efforts = self.value.as_ref().and_then(|value| {
            self.models
                .find(value)
                .filter(|model| crate::reasoning::selectable(&model.efforts))
                .map(|model| (value.effort, crate::reasoning::choices(&model.efforts)))
        });
        let features = self
            .value
            .as_ref()
            .and_then(|value| {
                let model = self.models.find(value)?;
                let declared = model.capabilities.as_ref()?.features.clone();
                Some(declared.into_iter().map(|feature| {
                    let current = value.features.as_ref().map(|features| {
                        features
                            .get(&feature.id)
                            .filter(|id| feature.options.iter().any(|option| option.id == **id))
                            .cloned()
                    });
                    (feature, current)
                }))
            })
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let owner = cx.entity().downgrade();
        let picker = self.picker.clone();
        let follow = self.follow;
        let choose = crate::theme::popover(Popover::new("team-model"), cx)
            .p_2()
            .anchor(Anchor::TopLeft)
            .overlay_closable(false)
            .trigger_style(StyleRefinement::default().w_full())
            .trigger(
                Button::new("team-model-trigger")
                    .outline()
                    .w_full()
                    .disabled(self.disabled)
                    .accessibility_label(tr("teams_model"))
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .text_sm()
                            .child(div().flex_1().min_w_0().truncate().child(title))
                            .child(IconName::ChevronDown),
                    ),
            )
            .on_open_change({
                let picker = picker.clone();
                move |open, _, cx| {
                    if *open {
                        picker.update(cx, |picker, cx| picker.reveal(cx));
                    }
                }
            })
            .content({
                let owner = owner.clone();
                move |_, _, cx| {
                    let popover = cx.entity().downgrade();
                    picker.update(cx, |picker, _| picker.popover = Some(popover.clone()));
                    let following = owner
                        .upgrade()
                        .is_some_and(|field| field.read(cx).value.is_none());
                    let clear = {
                        let owner = owner.clone();
                        let popover = popover.clone();
                        move |window: &mut Window, cx: &mut App| {
                            _ = owner.update(cx, |field, cx| {
                                if !field.disabled {
                                    field.set(None, cx);
                                }
                            });
                            _ = popover.update(cx, |state, cx| state.dismiss(window, cx));
                        }
                    };
                    let radio = clear.clone();
                    let content = v_flex()
                        .gap_1()
                        .child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .pr_3()
                                .child(
                                    Button::new("team-model-follow")
                                        .text()
                                        .flex_1()
                                        .min_w_0()
                                        .h_8()
                                        .px_2()
                                        .accessibility_label(tr(follow))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .truncate()
                                                .text_sm()
                                                .child(tr(follow)),
                                        )
                                        .on_click(move |_, window, cx| clear(window, cx)),
                                )
                                .child(
                                    Radio::new("team-model-follow-radio")
                                        .checked(following)
                                        .small()
                                        .w_4()
                                        .justify_center()
                                        .accessibility_label(tr(follow))
                                        .on_click(move |_, window, cx| radio(window, cx)),
                                ),
                        )
                        .child(Separator::horizontal())
                        .child(picker.clone());
                    Picker::surface(picker.clone(), popover, content)
                }
            });
        h_flex()
            .w_full()
            .gap_2()
            .child(div().flex_1().min_w_0().child(choose))
            .children(features.into_iter().map(|(feature, current)| {
                let owner = owner.clone();
                let label = feature_label(&feature, current.as_ref().map(Option::as_deref));
                let icon = if feature.id == "speed" {
                    Icon::new(gpui_kit::assets::IconName::Zap)
                } else {
                    feature
                        .icon
                        .as_ref()
                        .map(crate::assets::icons::icon)
                        .unwrap_or_else(|| IconName::Settings2.into())
                };
                Button::new(SharedString::from(format!(
                    "team-model-feature-{}",
                    feature.id
                )))
                .outline()
                .icon(icon)
                .label(label.clone())
                .dropdown_caret(true)
                .disabled(self.disabled)
                .accessibility_label(label)
                .dropdown_menu(move |menu, _, _| {
                    let choices = [None, Some(None)].into_iter().chain(
                        feature
                            .options
                            .iter()
                            .map(|option| Some(Some(option.id.clone()))),
                    );
                    choices.fold(menu, |menu, choice| {
                        let owner = owner.clone();
                        let id = feature.id.clone();
                        let checked = choice == current;
                        let item = feature_label(&feature, choice.as_ref().map(Option::as_deref));
                        menu.item(PopupMenuItem::new(item).checked(checked).on_click(
                            move |_, _, cx| {
                                let choice = choice.clone();
                                _ = owner.update(cx, |field, cx| field.feature(&id, choice, cx));
                            },
                        ))
                    })
                })
            }))
            .when_some(efforts, |row, (current, choices)| {
                row.child(
                    Button::new("team-model-effort")
                        .outline()
                        .label(effort_label(current))
                        .dropdown_caret(true)
                        .disabled(self.disabled)
                        .accessibility_label(effort_label(current))
                        .dropdown_menu({
                            let picker = self.picker.clone();
                            move |menu, _, cx| {
                                std::iter::once(None)
                                    .chain(choices.iter().copied().map(Some))
                                    .fold(menu, |menu, choice| {
                                        let owner = owner.clone();
                                        menu.item(
                                            if picker.read(cx).has_scores(cx) {
                                                crate::model_picker::effort_item(
                                                    effort_label(choice),
                                                    choice.and_then(|effort| {
                                                        picker.read(cx).effort_score(effort, cx)
                                                    }),
                                                    cx,
                                                )
                                            } else {
                                                PopupMenuItem::new(effort_label(choice))
                                            }
                                            .checked(choice == current)
                                            .on_click(
                                                move |_, _, cx| {
                                                    _ = owner.update(cx, |field, cx| {
                                                        field.effort(choice, cx)
                                                    });
                                                },
                                            ),
                                        )
                                    })
                            }
                        }),
                )
            })
    }
}
