//! Channel selection is presentation state; only the final action saves to the Node.
use super::super::presets;
use super::entry::brand_label;
use super::*;
use gpui_kit::component::{input::Input, tag::Tag};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Brand,
    Method,
    Configure,
}

pub(super) struct Flow {
    pub step: Step,
    pub brand: Option<String>,
    pub requested: Option<String>,
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
            requested: None,
            editing,
        }
    }

    pub(super) fn select(&mut self, brand: &str, count: usize) {
        if self.editing {
            return;
        }
        self.requested = None;
        self.brand = Some(brand.into());
        self.step = if count == 1 {
            Step::Configure
        } else {
            Step::Method
        };
    }

    pub(super) fn defer(&mut self, brand: String) {
        if !self.editing {
            self.requested = Some(brand);
        }
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

impl Editor {
    fn choices_count(&self) -> usize {
        self.flow
            .brand
            .as_ref()
            .and_then(|selected| {
                presets::groups(&self.directory)
                    .into_iter()
                    .find(|(key, _)| key == selected)
            })
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
            .child(self.directory_icon())
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().font_medium().child(self.selection_name()))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.method_label()),
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
        let selected = self.flow.brand.as_deref();
        let groups = presets::groups(&self.directory);
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
                        .filter(|(_, choices)| {
                            query.is_empty()
                                || brand_label(&choices[0]).to_lowercase().contains(&query)
                        })
                        .map(|(key, choices)| {
                            let first = &choices[0];
                            let name = brand_label(first);
                            let icon = presets::render_icon(first, rems(2.));
                            Button::new(SharedString::from(format!("provider-brand-{key}")))
                                .outline()
                                .w_full()
                                .h_auto()
                                .p_4()
                                .accessibility_label(name.clone())
                                .child(
                                    v_flex()
                                        .w_full()
                                        .min_w_0()
                                        .items_center()
                                        .gap_3()
                                        .child(icon)
                                        .child(div().w_full().truncate().text_sm().child(name)),
                                )
                                .on_click(cx.listener(move |editor, _, window, cx| {
                                    editor.choose_brand(key.clone(), window, cx);
                                }))
                        }),
                ),
            );
        } else if let Some((_, choices)) = groups
            .into_iter()
            .find(|(key, _)| Some(key.as_str()) == selected)
        {
            body = body.child(
                h_flex()
                    .gap_3()
                    .child(presets::render_icon(&choices[0], rems(2.)))
                    .child(brand_label(&choices[0])),
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
                    .children(choices.into_iter().map(|entry| {
                        let route = &entry.preset.routes[0];
                        let preset = presets::kind(&entry, route);
                        let login = matches!(entry.preset.auth, sailry_protocol::conversation::presets::Auth::Builtin { .. } | sailry_protocol::conversation::presets::Auth::Plugin { .. });
                        let label: SharedString = presets::label(&entry.preset).into();
                        Button::new(SharedString::from(format!(
                            "provider-method-{}",
                            entry.preset.id
                        )))
                        .outline()
                        .w_full()
                        .h_auto()
                        .p_4()
                        .accessibility_label(label.clone())
                        .child(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .gap_3()
                                .child(
                                    Icon::new(if login {
                                        IconName::CircleUser
                                    } else if preset.custom_endpoint() {
                                        IconName::Globe
                                    } else {
                                        IconName::Settings2
                                    })
                                    .size_5(),
                                )
                                .child(div().flex_1().min_w_0().text_left().child(label))
                                .when(entry.provenance == sailry_protocol::conversation::presets::Provenance::ThirdParty, |row| row.child(Tag::secondary().small().child(tr("provider_third_party"))))
                                .child(Icon::new(IconName::ChevronRight).size_4()),
                        )
                        .on_click(cx.listener(
                            move |editor, _, window, cx| editor.choose_entry(entry.clone(), window, cx),
                        ))
                    })),
            );
        }
        body.vertical_scrollbar(&self.scroll).into_any_element()
    }
}

#[cfg(test)]
mod tests;
