use super::{
    Workspace,
    group::{Group, Row},
};
use crate::{
    preferences::{self, MessageDisplay},
    tr,
};
use gpui_kit::{
    component::{
        slider::SliderState,
        tab::{Tab, TabBar},
    },
    *,
};

mod background;
mod sandbox;
mod text_size;

pub(super) struct State {
    pub text_size: Entity<SliderState>,
    preview: Entity<sandbox::Host>,
    background_preview: Entity<background::Host>,
    background_hosts: std::collections::BTreeMap<sailry_protocol::NodeId, Entity<background::Host>>,
    hosts: std::collections::BTreeMap<sailry_protocol::NodeId, Entity<sandbox::Host>>,
}

impl State {
    pub fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        let size = preferences::data(cx).message_text_size();
        let text_size = cx.new(|_| {
            SliderState::new()
                .min(12.)
                .max(18.)
                .step(2.)
                .default_value(f32::from(size))
        });
        // Kit accessibility actions set the value directly rather than emitting
        // pointer events. Observe the shared state so every interaction persists.
        cx.observe(&text_size, |_, slider, cx| {
            let value = slider.read(cx).value().start() as u8;
            if preferences::data(cx).message_text_size() != value {
                preferences::update(cx, |data| data.message_text_size = Some(value));
            }
            cx.notify();
        })
        .detach();
        Self {
            text_size,
            preview: cx.new(|cx| sandbox::Host::new(None, window, cx)),
            background_preview: cx.new(|cx| background::Host::new(None, window, cx)),
            background_hosts: Default::default(),
            hosts: Default::default(),
        }
    }
}

impl Workspace {
    pub(super) fn conversation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let host = if let Some(live) = &self.provider_link {
            let binding = live.binding.clone();
            let host = self
                .conversation
                .hosts
                .entry(binding.client.target())
                .or_insert_with(|| cx.new(|cx| sandbox::Host::new(Some(binding), window, cx)))
                .clone();
            if live.connected {
                host.update(cx, |host, cx| {
                    host.accept(live.sandbox_revision, window, cx)
                });
            }
            host
        } else {
            self.conversation.preview.clone()
        };
        let background = if let Some(live) = &self.provider_link {
            let binding = live.binding.clone();
            let host = self
                .conversation
                .background_hosts
                .entry(binding.client.target())
                .or_insert_with(|| cx.new(|cx| background::Host::new(Some(binding), window, cx)))
                .clone();
            if live.connected {
                host.update(cx, |host, cx| {
                    host.accept(live.background_revision, window, cx)
                });
            }
            host
        } else {
            self.conversation.background_preview.clone()
        };
        let display = preferences::data(cx).message_display.unwrap_or_default();
        gpui_kit::component::v_flex()
            .gap_6()
            .child(host)
            .child(background)
            .child(
                Group::new("settings_messages")
                    .child(
                        Row::new(
                            "settings_message_display",
                            TabBar::new("message-display")
                                .segmented()
                                .selected_index(usize::from(display == MessageDisplay::Compact))
                                .children(
                                    ["message_display_detailed", "message_display_compact"].map(
                                        |key| {
                                            Tab::new()
                                                .debug_selector(move || key.into())
                                                .label(tr(key))
                                        },
                                    ),
                                )
                                .on_click(|index, _, cx| {
                                    preferences::update(cx, |data| {
                                        data.message_display = Some(
                                            [MessageDisplay::Detailed, MessageDisplay::Compact]
                                                [*index],
                                        );
                                    });
                                }),
                        )
                        .description("settings_message_display_description"),
                    )
                    .child(
                        Row::new(
                            "settings_message_text_size",
                            text_size::slider(&self.conversation.text_size, cx),
                        )
                        .wide(),
                    ),
            )
            .into_any_element()
    }
}
