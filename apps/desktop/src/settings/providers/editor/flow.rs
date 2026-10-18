//! Channel selection is presentation state; only the final action saves to the Node.
use super::*;
use gpui_kit::component::input::Input;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Brand,
    Method,
    Configure,
}

pub(super) struct Flow {
    pub step: Step,
    pub brand: Option<&'static str>,
    editing: bool,
}

impl Flow {
    pub fn new(editing: bool) -> Self {
        Self {
            step: if editing {
                Step::Configure
            } else {
                Step::Brand
            },
            brand: None,
            editing,
        }
    }

    pub(super) fn select(&mut self, brand: &'static str, count: usize) {
        if self.editing {
            return;
        }
        self.brand = Some(brand);
        self.step = if count == 1 {
            Step::Configure
        } else {
            Step::Method
        };
    }

    fn back(&mut self, count: usize) {
        if self.editing {
            return;
        }
        self.step = match self.step {
            Step::Configure if count > 1 => Step::Method,
            _ => Step::Brand,
        };
    }
}

fn brand(preset: Preset) -> &'static str {
    if matches!(preset, Preset::OpenCodeGo | Preset::OpenCodeZen) {
        "provider_opencode"
    } else {
        preset.family().key()
    }
}

fn icon(preset: Preset) -> Icon {
    preset.category().icon()
}

fn label(preset: Preset) -> SharedString {
    tr(
        if matches!(
            preset,
            Preset::OpenAi | Preset::Anthropic | Preset::Gemini | Preset::Hosted(_)
        ) {
            "provider_api_key"
        } else {
            preset.key()
        },
    )
}

impl Editor {
    fn choices_count(&self) -> usize {
        let selected = self.flow.brand.unwrap_or_else(|| brand(self.preset));
        kind::groups()
            .into_iter()
            .find(|(key, _)| *key == selected)
            .map_or(0, |(_, choices)| choices.len())
    }

    pub(super) fn navigation_blocked(&self) -> bool {
        self.pending || self.closed || self.probe.is_some()
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        if self.navigation_blocked() {
            return;
        }
        self.flow.back(self.choices_count());
        self.scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    pub(super) fn choose(&mut self, preset: Preset, window: &mut Window, cx: &mut Context<Self>) {
        if self.navigation_blocked() || self.editing.is_some() {
            return;
        }
        let changed = preset.kind() != self.preset.kind();
        self.select_preset(preset, window, cx);
        if changed || self.name.read(cx).value().trim().is_empty() {
            self.name.update(cx, |input, cx| {
                input.set_value(tr(preset.key()), window, cx)
            });
        }
        self.flow.step = Step::Configure;
        self.load_partners(window, cx);
        self.scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    pub(super) fn heading(&self, cx: &Context<Self>) -> AnyElement {
        h_flex()
            .debug_selector(|| "provider-dialog-title".into())
            .w_full()
            .min_w_0()
            .gap_2()
            .when(
                self.editing.is_none() && self.flow.step != Step::Brand,
                |row| {
                    row.child(
                        Button::new("provider-back")
                            .ghost()
                            .small()
                            .icon(IconName::ArrowLeft)
                            .accessibility_label(tr("provider_step_back"))
                            .disabled(self.navigation_blocked())
                            .on_click(cx.listener(|editor, _, _, cx| editor.back(cx))),
                    )
                },
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(tr(if self.editing.is_some() {
                        "provider_edit_title"
                    } else {
                        match self.flow.step {
                            Step::Brand => "provider_add_title",
                            Step::Method => "provider_choose_method",
                            Step::Configure => "provider_configure",
                        }
                    })),
            )
            .into_any_element()
    }

    pub(super) fn selection(&self, cx: &Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .child(icon(self.preset).size_8())
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .font_medium()
                            .child(tr(self.flow.brand.unwrap_or_else(|| brand(self.preset)))),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(label(self.preset)),
                    ),
            )
            .when(self.editing.is_none(), |row| {
                row.child(
                    Button::new("provider-change")
                        .ghost()
                        .small()
                        .label(tr("provider_change"))
                        .disabled(self.navigation_blocked())
                        .on_click(cx.listener(|editor, _, _, cx| {
                            if !editor.navigation_blocked() {
                                editor.flow.step = Step::Brand;
                                cx.notify();
                            }
                        })),
                )
            })
    }

    pub(super) fn choices(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let width = (window.viewport_size().width - px(96.)).min(px(712.));
        let selected = self.flow.brand;
        let groups = kind::groups();
        let mut body = v_flex()
            .id("provider-choices")
            .w_full()
            .min_h_0()
            .gap_4()
            .overflow_y_scroll()
            .track_scroll(&self.scroll);
        if self.flow.step == Step::Brand {
            let query = self.search.read(cx).value().trim().to_lowercase();
            let columns = if width >= px(640.) {
                5
            } else if width >= px(500.) {
                4
            } else if width >= px(360.) {
                3
            } else {
                2
            };
            body = body.child(
                Input::new(&self.search)
                    .w_full()
                    .prefix(IconName::Search)
                    .aria_label(tr("provider_search")),
            );
            body = body.child(
                div().grid().grid_cols(columns).w_full().gap_3().children(
                    groups
                        .into_iter()
                        .filter(|(key, _)| {
                            query.is_empty() || tr(key).to_lowercase().contains(&query)
                        })
                        .map(|(key, choices)| {
                            let preset = choices[0];
                            Button::new(SharedString::from(format!("provider-brand-{key}")))
                                .outline()
                                .w_full()
                                .h_auto()
                                .p_4()
                                .accessibility_label(tr(key))
                                .child(
                                    v_flex()
                                        .w_full()
                                        .min_w_0()
                                        .items_center()
                                        .gap_3()
                                        .child(icon(preset).size_8())
                                        .child(div().w_full().truncate().text_sm().child(tr(key))),
                                )
                                .on_click(cx.listener(move |editor, _, window, cx| {
                                    editor.choose_brand(key, window, cx);
                                }))
                        }),
                ),
            );
        } else if let Some((key, choices)) =
            groups.into_iter().find(|(key, _)| Some(*key) == selected)
        {
            body = body.child(
                h_flex()
                    .gap_3()
                    .child(icon(choices[0]).size_8())
                    .child(tr(key)),
            );
            body = body.child(
                div()
                    .grid()
                    .w_full()
                    .gap_3()
                    .grid_cols(if choices.len() > 4 && width >= px(480.) {
                        2
                    } else {
                        1
                    })
                    .children(choices.into_iter().map(|preset| {
                        Button::new(SharedString::from(format!(
                            "provider-method-{}",
                            preset.key()
                        )))
                        .outline()
                        .w_full()
                        .h_auto()
                        .p_4()
                        .accessibility_label(label(preset))
                        .child(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .gap_3()
                                .child(
                                    Icon::new(if preset.oauth() {
                                        IconName::CircleUser
                                    } else if preset.custom_endpoint() {
                                        IconName::Globe
                                    } else {
                                        IconName::Settings2
                                    })
                                    .size_5(),
                                )
                                .child(div().flex_1().min_w_0().text_left().child(label(preset)))
                                .child(Icon::new(IconName::ChevronRight).size_4()),
                        )
                        .on_click(cx.listener(
                            move |editor, _, window, cx| editor.choose(preset, window, cx),
                        ))
                    })),
            );
        }
        body.vertical_scrollbar(&self.scroll).into_any_element()
    }
}

#[cfg(test)]
mod tests;
