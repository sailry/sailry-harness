mod host;
#[cfg(test)]
mod tests;
use super::{
    Workspace,
    group::{Group, Row},
};
use crate::tr;
use gpui_kit::{
    component::{
        input::{Input, InputEvent, InputState},
        slider::{Slider, SliderEvent, SliderState},
        switch::Switch,
        *,
    },
    *,
};
use sailry_protocol::NodeId;
use std::collections::BTreeMap;

pub(super) struct State {
    pub preview: Entity<host::Host>,
    hosts: BTreeMap<NodeId, Entity<host::Host>>,
    pub font_size: Entity<SliderState>,
    font: Entity<InputState>,
}

impl State {
    pub fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        let preferences = crate::preferences::data(cx).terminal;
        let state = Self {
            preview: cx.new(|cx| host::Host::new(None, window, cx)),
            hosts: BTreeMap::new(),
            font: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(preferences.font_family)
                    .placeholder(cx.theme().mono_font_family.clone())
            }),
            font_size: cx.new(|_| {
                SliderState::new()
                    .min(8.)
                    .max(32.)
                    .step(1.)
                    .default_value(preferences.font_size as f32)
            }),
        };
        cx.subscribe(&state.font, |_, input, event, cx| {
            if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                let value = input.read(cx).value().trim().to_owned();
                if crate::preferences::data(cx).terminal.font_family != value {
                    crate::preferences::update(cx, |data| data.terminal.font_family = value);
                }
            }
        })
        .detach();
        cx.subscribe(&state.font_size, |_, _, event, cx| {
            if let SliderEvent::Release(value) = event {
                let size = value.start() as u8;
                if crate::preferences::data(cx).terminal.font_size != size {
                    crate::preferences::update(cx, |data| data.terminal.font_size = size);
                }
            }
        })
        .detach();
        state
    }
}

impl Workspace {
    pub(super) fn terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let host = if let Some(live) = &self.provider_link {
            let binding = live.binding.clone();
            let host = self
                .terminal
                .hosts
                .entry(binding.client.target())
                .or_insert_with(|| cx.new(|cx| host::Host::new(Some(binding), window, cx)))
                .clone();
            if live.connected {
                host.update(cx, |host, cx| {
                    host.accept(live.terminal_revision, window, cx)
                });
            }
            host
        } else {
            self.terminal.preview.clone()
        };
        let preferences = crate::preferences::data(cx).terminal;
        v_flex()
            .gap_6()
            .child(host)
            .child(
                Group::new("terminal_input").child(
                    Row::new(
                        "settings_paste_protection",
                        Switch::new("paste-protection")
                            .checked(preferences.paste_protection)
                            .accessibility_label(tr("settings_paste_protection"))
                            .on_click(|value, _, cx| {
                                crate::preferences::update(cx, |data| {
                                    data.terminal.paste_protection = *value
                                })
                            }),
                    )
                    .description("settings_paste_description"),
                ),
            )
            .child(
                Group::new("settings_terminal_display")
                    .child(
                        Row::new(
                            "settings_font_size",
                            h_flex()
                                .w_full()
                                .gap_3()
                                .child(Slider::new(&self.terminal.font_size))
                                .child(
                                    self.terminal.font_size.read(cx).value().start().to_string(),
                                ),
                        )
                        .wide(),
                    )
                    .child(
                        Row::new(
                            "settings_font_family",
                            Input::new(&self.terminal.font).aria_label(tr("settings_font_family")),
                        )
                        .wide(),
                    ),
            )
            .into_any_element()
    }
}
