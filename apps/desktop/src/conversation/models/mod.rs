pub(crate) use crate::model_picker::{Picker, Selection};

use crate::{
    settings::{Channel, Model},
    shell::Shell,
    tr,
};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
    popover::Popover,
    *,
};
use gpui_kit::*;

impl Shell {
    pub(crate) fn selected_model(&self, cx: &App) -> Option<(Channel, Model)> {
        self.selected_model_for((self.host, self.session), cx)
    }

    pub(super) fn selected_model_for(
        &self,
        key: (usize, usize),
        cx: &App,
    ) -> Option<(Channel, Model)> {
        let channels = self.settings.read(cx).model_channels();
        let selected = &self.conversations[&key].options.model;
        let channel = match selected {
            Some(selection) => channels
                .iter()
                .find(|c| c.enabled && c.id == selection.channel),
            None => channels.iter().find(|c| c.enabled && !c.models.is_empty()),
        }?;
        let model = match selected {
            Some(selection) => channel.models.iter().find(|m| m.id == selection.model),
            None => channel
                .models
                .iter()
                .find(|m| m.id == channel.default_model)
                .or(channel.models.first()),
        }?;
        Some((channel.clone(), model.clone()))
    }

    pub(super) fn composer_model(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let label = self
            .selected_model(cx)
            .map(|(_, model)| model.id.into())
            .unwrap_or_else(|| tr("composer_select_model"));
        let picker = self.model_picker.clone();
        let selected = self.selected_model(cx).map(|(channel, model)| Selection {
            channel: channel.id,
            model: model.id,
        });
        let channels = self.settings.read(cx).model_channels().to_vec();
        picker.update(cx, |picker, cx| picker.set(channels, selected, cx));
        let opening = picker.clone();
        Popover::new("composer-model-popover")
            .p_2()
            .anchor(Anchor::BottomRight)
            .trigger(
                Button::new("composer-model")
                    .custom(crate::theme::subtle_button(cx))
                    .rounded_full()
                    .debug_selector(|| "composer-model".into())
                    .label(label)
                    .max_w(px(200.))
                    .tooltip(tr("composer_model_hint")),
            )
            .on_open_change(move |open, _, cx| {
                if *open {
                    opening.update(cx, |picker, cx| picker.reveal(cx));
                }
            })
            .content(move |_, _, cx| {
                let popover = cx.entity().downgrade();
                picker.update(cx, |picker, _| {
                    picker.popover = Some(popover);
                });
                picker.clone()
            })
    }

    pub(super) fn composer_effort(&self, cx: &mut Context<Self>) -> AnyElement {
        let key = (self.host, self.session);
        let model = self.selected_model(cx).map(|(_, model)| model);
        let options = model
            .as_ref()
            .filter(|m| m.reasoning)
            .map(|m| crate::reasoning::choices(&m.efforts))
            .unwrap_or_default();
        if !crate::reasoning::selectable(&options) {
            return div().into_any_element();
        }
        let current = self.conversations[&key]
            .options
            .effort
            .as_ref()
            .filter(|effort| options.contains(effort))
            .cloned()
            .or_else(|| {
                model
                    .as_ref()
                    .map(|m| sailry_protocol::Effort::initial(&m.efforts, m.default_effort))
            });
        let owner = cx.entity().downgrade();
        Button::new("composer-effort")
            .custom(crate::theme::subtle_button(cx))
            .rounded_full()
            .debug_selector(|| "composer-effort".into())
            .label(
                current
                    .filter(|_| !options.is_empty())
                    .map(crate::reasoning::label)
                    .unwrap_or_else(|| tr("composer_effort")),
            )
            .tooltip(tr("composer_effort"))
            .disabled(options.is_empty())
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _, _| {
                options.iter().fold(menu, |menu, effort| {
                    let value = *effort;
                    let owner = owner.clone();
                    menu.item(
                        PopupMenuItem::new(crate::reasoning::label(*effort))
                            .checked(current.as_ref() == Some(effort))
                            .on_click(move |_, _, cx| {
                                _ = owner.update(cx, |owner, cx| {
                                    if let Some(conversation) = owner.conversations.get_mut(&key) {
                                        conversation.options.effort = Some(value);
                                        cx.notify();
                                    }
                                });
                            }),
                    )
                })
            })
            .into_any_element()
    }
}
