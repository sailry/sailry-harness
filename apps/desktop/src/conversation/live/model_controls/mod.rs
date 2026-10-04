//! One composer popover for model choice and discrete reasoning levels.
use super::*;
use crate::model_picker::Picker;
use gpui_kit::base::{Transition, transition};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    popover::Popover,
    slider::{SliderEvent, SliderState},
};
use gpui_kit::prelude::FluentBuilder as _;

#[cfg(test)]
mod tests;
mod track;

impl View {
    fn sync_model_controls(&self, cx: &mut Context<Self>) -> bool {
        let selected = self.config.as_ref().and_then(|config| {
            Some(super::super::models::Selection {
                channel: *self
                    .provider_ids
                    .get(&(self.config_owner, config.provider))?,
                model: config.model.clone(),
            })
        });
        let channels = self
            .model_sources()
            .filter_map(|(node, provider)| {
                let id = self.provider_ids.get(&(node, provider.id))?;
                let mut channel = crate::settings::channel(*id, provider);
                channel.enabled &= self.source_connected(node);
                if let Some(profile) = self
                    .session
                    .as_ref()
                    .and_then(|session| session.profile.as_ref())
                {
                    channel.name = rust_i18n::t!(
                        if profile.provider.id == provider.id {
                            "chat_provider_session"
                        } else {
                            "chat_provider_node"
                        },
                        name = provider.name,
                    )
                    .to_string();
                } else if self.session.is_none()
                    && self.binding.client.target() != self.binding.defaults.target()
                {
                    let source = if node == self.binding.defaults.target() {
                        tr("chat_config_desktop")
                    } else {
                        self.binding.host.clone()
                    };
                    channel.name = rust_i18n::t!(
                        "chat_provider_source",
                        name = provider.name,
                        source = source
                    )
                    .to_string();
                }
                Some(channel)
            })
            .collect::<Vec<_>>();
        let choices = self.available_efforts();
        let model = self.config.as_ref().map_or_else(
            || tr("composer_select_model"),
            |config| config.model.clone().into(),
        );
        let effort = self
            .config
            .as_ref()
            .map_or(Effort::Default, |config| config.effort);
        let initial = self
            .config
            .as_ref()
            .and_then(|config| {
                self.providers()
                    .find(|provider| provider.id == config.provider)?
                    .models
                    .iter()
                    .find(|model| model.id == config.model)
            })
            .map_or(Effort::Default, |model| {
                Effort::initial(&model.efforts, model.default_effort)
            });
        let saving = self.saving_config();
        let available = channels
            .iter()
            .any(|channel| channel.enabled && !channel.models.is_empty());
        self.model_picker.update(cx, |picker, cx| {
            picker.set(channels, selected, cx);
            picker.popover = None;
            picker.composer(effort, self.busy() || !self.can_pick_model(), saving, cx);
        });
        let panel = self.model_controls.clone();
        panel.update(cx, |panel, cx| {
            panel.sync(
                Settings {
                    model,
                    choices,
                    effort,
                    initial,
                    busy: self.busy() || !self.can_pick_model(),
                    connected: self.configuration().connected,
                },
                cx,
            )
        });
        available
    }

    pub(super) fn model_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.sync_model_controls(cx) {
            return self.model_controls.clone().into_any_element();
        }
        Button::new("composer-select-model")
            .ghost()
            .w_full()
            .label(tr("composer_select_model"))
            .disabled(self.busy() || !self.can_pick_model())
            .on_click(cx.listener(|view, _, _, cx| cx.emit(Event::Settings(view.settings_node()))))
            .into_any_element()
    }

    pub(super) fn model_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let available = self.sync_model_controls(cx);
        let model = self.config.as_ref().map_or_else(
            || tr("composer_select_model"),
            |config| config.model.clone().into(),
        );
        let effort = self
            .config
            .as_ref()
            .map_or(Effort::Default, |config| config.effort);
        let choices = self.available_efforts();
        let saving = self.saving_config();
        let button = Button::new("live-model")
            .custom(crate::theme::subtle_button(cx))
            .rounded_full()
            .max_w(px(260.))
            .when(self.sidebar, |button| button.max_w(px(220.)).min_w_0())
            .debug_selector(|| "live-chat-model".into())
            .accessibility_label(tr("composer_model_settings"))
            .child(
                h_flex()
                    .min_w_0()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .line_height(px(20.))
                            .child(model.clone()),
                    )
                    .when(!choices.is_empty(), |row| {
                        row.child(
                            div()
                                .debug_selector(|| "live-chat-effort".into())
                                .flex_shrink_0()
                                .text_sm()
                                .line_height(px(20.))
                                .text_color(cx.theme().muted_foreground)
                                .child(crate::reasoning::label(effort)),
                        )
                    }),
            )
            .disabled(self.busy() || !self.can_pick_model())
            .when(saving && self.can_pick_model(), |button| {
                button.text_color(cx.theme().foreground)
            });
        if !available {
            return button
                .on_click(
                    cx.listener(|view, _, _, cx| cx.emit(Event::Settings(view.settings_node()))),
                )
                .into_any_element();
        }
        let panel = self.model_controls.clone();
        let opening = panel.clone();
        Popover::new("live-model-popover")
            .anchor(Anchor::BottomRight)
            .bottom_2()
            .p_3()
            .trigger(button)
            .on_open_change(move |open, window, cx| {
                opening.update(cx, |panel, cx| {
                    panel.models = false;
                    panel.dragging = false;
                    if *open {
                        panel.focus.focus(window, cx);
                    }
                    cx.notify();
                });
            })
            .content(move |_, _, _| panel.clone())
            .into_any_element()
    }
}

#[derive(Clone, PartialEq)]
struct Settings {
    model: SharedString,
    choices: Vec<Effort>,
    effort: Effort,
    initial: Effort,
    busy: bool,
    connected: bool,
}

pub(super) struct Panel {
    owner: WeakEntity<View>,
    picker: Entity<Picker>,
    slider: Entity<SliderState>,
    focus: FocusHandle,
    settings: Option<Settings>,
    models: bool,
    dragging: bool,
}

impl Panel {
    pub(super) fn new(
        owner: WeakEntity<View>,
        picker: Entity<Picker>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let slider = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(1.)
                .step(1.)
                .default_value(0.)
        });
        cx.subscribe_in(&slider, window, |panel, _, event, window, cx| match event {
            SliderEvent::Change(_) => {
                panel.dragging = true;
                cx.notify();
            }
            SliderEvent::Release(value) => {
                panel.dragging = false;
                panel.apply(value.end() as usize, window, cx);
            }
        })
        .detach();
        cx.subscribe_in(
            &picker,
            window,
            |panel, _, _: &crate::model_picker::Back, window, cx| {
                panel.back(window, cx);
            },
        )
        .detach();
        Self {
            owner,
            picker,
            slider,
            focus: cx.focus_handle(),
            settings: None,
            models: false,
            dragging: false,
        }
    }

    fn sync(&mut self, mut settings: Settings, cx: &mut Context<Self>) {
        settings.choices.sort_by_key(|choice| order(*choice));
        if self.settings.as_ref() != Some(&settings) {
            self.settings = Some(settings);
            cx.notify();
        }
    }

    pub(super) fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.models = false;
        self.dragging = false;
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn apply(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(settings) = &self.settings else {
            return;
        };
        if settings.busy || !settings.connected {
            return;
        }
        let Some(effort) = settings.choices.get(index).copied() else {
            return;
        };
        let _ = self
            .owner
            .update(cx, |view, cx| view.select_effort(effort, window, cx));
        cx.notify();
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(settings) = &self.settings else {
            return;
        };
        if self.models || settings.busy || !settings.connected || settings.choices.len() < 2 {
            return;
        }
        let current = self.slider.read(cx).value().end() as usize;
        let last = settings.choices.len() - 1;
        let index = match event.keystroke.key.as_str() {
            "left" | "down" => current.saturating_sub(1),
            "right" | "up" => (current + 1).min(last),
            "home" => 0,
            "end" => last,
            _ => return,
        };
        cx.stop_propagation();
        self.slider
            .update(cx, |slider, cx| slider.set_value(index as f32, window, cx));
        self.apply(index, window, cx);
    }
}

impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(settings) = self.settings.clone() else {
            return div().into_any_element();
        };
        let size = crate::model_picker::panel_size(window);
        let height = transition(
            "model-controls-height",
            if self.models { size.height } else { px(104.) },
            Transition::new(cx.theme().motion_tokens().duration_normal)
                .easing(cx.theme().motion_tokens().easing_move.clone()),
            window,
            cx,
        );
        let body = v_flex()
            .id("model-controls")
            .debug_selector(|| "model-controls".into())
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .w(size.width)
            .h(height)
            .overflow_hidden()
            .items_center()
            .text_sm()
            .gap_2();
        if self.models {
            return body.child(self.picker.clone()).into_any_element();
        }
        let current = settings
            .choices
            .iter()
            .position(|effort| *effort == settings.effort)
            .unwrap_or(0);
        let max = settings.choices.len().saturating_sub(1).max(1) as f32;
        if self.slider.read(cx).max_value() != max {
            self.dragging = false;
            self.slider.update(cx, |slider, _| {
                *slider = SliderState::new()
                    .min(0.)
                    .max(max)
                    .step(1.)
                    .default_value(current as f32)
            });
        }
        if !self.dragging && !settings.busy && self.slider.read(cx).value().end() != current as f32
        {
            self.slider.update(cx, |slider, cx| {
                slider.set_value(current as f32, window, cx)
            });
        }
        let index = self.slider.read(cx).value().end() as usize;
        let selected = settings
            .choices
            .get(index)
            .copied()
            .unwrap_or(settings.effort);
        let highest = settings.choices.len() > 1
            && index + 1 == settings.choices.len()
            && !matches!(
                selected,
                Effort::Default | Effort::Disabled | Effort::Budget(-1)
            );
        let color = if highest {
            cx.theme().warning
        } else {
            cx.theme().primary
        };
        let controls = v_flex()
            .w_full()
            .flex_shrink_0()
            .gap_2()
            .child(
                v_flex()
                    .w_full()
                    .min_w_0()
                    .items_center()
                    .gap_1()
                    .child(
                        h_flex()
                            .relative()
                            .w_full()
                            .h_6()
                            .justify_center()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(color)
                                    .debug_selector(|| "model-controls-effort".into())
                                    .child(if settings.choices.is_empty() {
                                        tr("composer_effort_unavailable")
                                    } else {
                                        crate::reasoning::label(selected)
                                    }),
                            )
                            .child(
                                Button::new("model-controls-reset")
                                    .rounded_full()
                                    .text()
                                    .small()
                                    .absolute()
                                    .right_2()
                                    .icon(IconName::RotateCw)
                                    .tooltip(tr("composer_effort_reset"))
                                    .accessibility_label(tr("composer_effort_reset"))
                                    .disabled(
                                        settings.busy
                                            || !settings.connected
                                            || !settings.choices.contains(&settings.initial)
                                            || selected == settings.initial,
                                    )
                                    .debug_selector(|| "model-controls-reset".into())
                                    .on_click(cx.listener(|panel, _, window, cx| {
                                        let index = panel.settings.as_ref().and_then(|settings| {
                                            settings
                                                .choices
                                                .iter()
                                                .position(|choice| *choice == settings.initial)
                                        });
                                        if let Some(index) = index {
                                            panel.apply(index, window, cx);
                                        }
                                    })),
                            ),
                    )
                    .child(
                        Button::new("model-controls-choose")
                            .rounded_full()
                            .text()
                            .small()
                            .h_7()
                            .max_w_full()
                            .accessibility_label(settings.model.clone())
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .line_height(px(20.))
                                    .child(settings.model),
                            )
                            .child(Icon::new(IconName::ChevronRight).size_3())
                            .disabled(settings.busy)
                            .debug_selector(|| "model-controls-choose".into())
                            .on_click(cx.listener(|panel, _, _, cx| {
                                panel.models = true;
                                panel.picker.update(cx, |picker, cx| picker.reveal(cx));
                                cx.notify();
                            })),
                    ),
            )
            .child(track::render(
                &self.slider,
                &self.focus,
                settings.choices.len(),
                highest,
                settings.busy || !settings.connected || settings.choices.len() < 2,
                cx,
            ));
        body.child(controls).into_any_element()
    }
}

fn order(effort: Effort) -> (u8, i32) {
    match effort {
        Effort::Default | Effort::Budget(-1) => (0, -1),
        Effort::Disabled => (0, 0),
        Effort::Minimal => (1, 0),
        Effort::Low => (2, 0),
        Effort::Medium => (3, 0),
        Effort::High => (4, 0),
        Effort::XHigh => (5, 0),
        Effort::Max => (6, 0),
        Effort::Budget(value) => (7, value),
    }
}
