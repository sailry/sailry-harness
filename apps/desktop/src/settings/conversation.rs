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
        slider::{Slider, SliderState},
        tab::{Tab, TabBar},
        *,
    },
    *,
};

pub(super) struct State {
    pub text_size: Entity<SliderState>,
}

const SIZE_LABELS: [&str; 4] = [
    "message_text_small",
    "message_text_standard",
    "message_text_large",
    "message_text_extra_large",
];

impl State {
    pub fn new(cx: &mut Context<Workspace>) -> Self {
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
        Self { text_size }
    }
}

impl Workspace {
    pub(super) fn conversation(&self, cx: &mut Context<Self>) -> AnyElement {
        let display = preferences::data(cx).message_display.unwrap_or_default();
        Group::new("settings_messages")
            .child(
                Row::new(
                    "settings_message_display",
                    TabBar::new("message-display")
                        .segmented()
                        .selected_index(usize::from(display == MessageDisplay::Compact))
                        .children(["message_display_detailed", "message_display_compact"].map(
                            |key| Tab::new().debug_selector(move || key.into()).label(tr(key)),
                        ))
                        .on_click(|index, _, cx| {
                            preferences::update(cx, |data| {
                                data.message_display = Some(
                                    [MessageDisplay::Detailed, MessageDisplay::Compact][*index],
                                );
                            });
                        }),
                )
                .description("settings_message_display_description"),
            )
            .child(
                Row::new(
                    "settings_message_text_size",
                    v_flex()
                        .w_full()
                        .min_w_0()
                        .gap_2()
                        .child(
                            div()
                                .id("message-text-size-slider")
                                .w_full()
                                .px_2()
                                .aria_label(tr("settings_message_text_size"))
                                .debug_selector(|| "message-text-size-slider".into())
                                .child(Slider::new(&self.conversation.text_size)),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .justify_between()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .children(
                                    crate::preferences::Data::MESSAGE_TEXT_SIZES
                                        .into_iter()
                                        .zip(SIZE_LABELS)
                                        .map(|(size, label)| {
                                            v_flex()
                                                .flex_1()
                                                .min_w_0()
                                                .items_center()
                                                .text_center()
                                                .debug_selector(move || {
                                                    format!("message-text-size-label-{size}")
                                                })
                                                .child(tr(label))
                                                .child(
                                                    rust_i18n::t!(
                                                        "message_text_size_value",
                                                        size = size
                                                    )
                                                    .to_string(),
                                                )
                                        }),
                                ),
                        ),
                )
                .wide(),
            )
            .into_any_element()
    }
}
