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
        tooltip::Tooltip,
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
        let text_size = self.conversation.text_size.clone();
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
                    div()
                        .id("message-text-size-slider")
                        .relative()
                        .w_full()
                        .min_w_0()
                        .px_2()
                        .aria_label(tr("settings_message_text_size"))
                        .debug_selector(|| "message-text-size-slider".into())
                        .tooltip(move |window, cx| {
                            let size = text_size.read(cx).value().start() as u8;
                            let label = SIZE_LABELS[usize::from((size - 12) / 2)];
                            Tooltip::element(move |_, _| {
                                div()
                                    .debug_selector(|| "message-text-size-tooltip".into())
                                    .child(format!(
                                        "{} · {}",
                                        tr(label),
                                        rust_i18n::t!("message_text_size_value", size = size)
                                    ))
                            })
                            .build(window, cx)
                        })
                        .child(
                            div().absolute().left_2().right_2().top_0().h_6().children(
                                // Kit supplies discrete interaction but no tick API.
                                // These eventless marks leave its thumb and track intact.
                                crate::preferences::Data::MESSAGE_TEXT_SIZES
                                    .into_iter()
                                    .map(|size| {
                                        div()
                                            .absolute()
                                            .left(relative(f32::from(size - 12) / 6.))
                                            .top_2()
                                            .ml(-px(0.5))
                                            .w(px(1.))
                                            .h_2()
                                            .bg(cx.theme().tokens.slider_bar.opacity(0.5))
                                            .debug_selector(move || {
                                                format!("message-text-size-tick-{size}")
                                            })
                                    }),
                            ),
                        )
                        .child(Slider::new(&self.conversation.text_size)),
                )
                .wide(),
            )
            .into_any_element()
    }
}
