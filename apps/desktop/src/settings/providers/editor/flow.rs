//! Channel selection is presentation state; only the final action saves to the Node.
use super::super::presets;
use super::entry::brand_label;
use super::*;
use gpui_kit::component::{button::ButtonCustomVariant, input::Input};

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

/// How a method connects, shown as its icon and one-line description.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Access {
    Key,
    Endpoint,
    Login,
    Cloud,
}

impl Access {
    fn of(kind: Preset, login: bool, custom_endpoint: bool) -> Self {
        if login {
            Self::Login
        } else if kind.cloud() {
            Self::Cloud
        } else if custom_endpoint {
            Self::Endpoint
        } else {
            Self::Key
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Key => "provider_access_key",
            Self::Endpoint => "provider_access_endpoint",
            Self::Login => "provider_access_login",
            Self::Cloud => "provider_access_cloud",
        }
    }

    fn icon(self) -> Icon {
        use gpui_kit::assets::IconName as Glyph;
        match self {
            Self::Key => Icon::new(Glyph::KeyRound),
            Self::Endpoint => Icon::new(Glyph::Link),
            Self::Login => Icon::new(IconName::CircleUser),
            Self::Cloud => Icon::new(Glyph::Cloud),
        }
    }
}

/// Second summary line: the method name, or its access kind when the name
/// would only repeat the family.
fn method_summary(family: &SharedString, method: SharedString, access: Access) -> SharedString {
    if method == *family {
        tr(access.key())
    } else {
        method
    }
}

/// Framed choice surface. Kit derives a custom variant's border from its color
/// in every state, so the theme border stays stable while hover uses the list token.
fn surface(button: Button, cx: &App) -> Button {
    button
        .custom(
            ButtonCustomVariant::new(cx)
                .color(cx.theme().border)
                .foreground(cx.theme().foreground)
                .hover(cx.theme().list_hover)
                .active(cx.theme().list_active),
        )
        .w_full()
        .min_w_0()
        .border_1()
        .bg(cx.theme().transparent)
}

fn icon_tile(size: Pixels, icon: impl IntoElement, cx: &App) -> Div {
    div()
        .size(size)
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(cx.theme().radius)
        .bg(cx.theme().muted)
        .child(icon)
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
        // A fixed row height keeps every step's title level with the close button.
        h_flex()
            .debug_selector(|| "provider-dialog-title".into())
            .w_full()
            .min_w_0()
            .h_6()
            .pr_6()
            .items_center()
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
                    .truncate()
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
        let family = self.selection_name();
        let method = method_summary(
            &family,
            self.method_label(),
            Access::of(self.preset, self.uses_login(), self.custom_endpoint()),
        );
        h_flex()
            .w_full()
            .min_w_0()
            .items_center()
            .gap_3()
            .p_3()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius_lg)
            .bg(cx.theme().secondary)
            .child(icon_tile(px(40.), self.directory_icon(), cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(div().truncate().font_medium().child(family))
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(method),
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
            .gap_3()
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
                            surface(
                                Button::new(SharedString::from(format!("provider-brand-{key}"))),
                                cx,
                            )
                            .h(px(92.))
                            .p_2()
                            .accessibility_label(name.clone())
                            .child(
                                v_flex()
                                    .size_full()
                                    .min_w_0()
                                    .items_center()
                                    .justify_center()
                                    .gap_2()
                                    .child(presets::render_icon(first, rems(1.75)))
                                    .child(
                                        div()
                                            .max_w_full()
                                            .truncate()
                                            .text_center()
                                            .text_sm()
                                            .child(name),
                                    ),
                            )
                            .on_click(cx.listener(
                                move |editor, _, window, cx| {
                                    editor.choose_brand(key.clone(), window, cx);
                                },
                            ))
                        }),
                ),
            );
        } else if let Some((_, choices)) = groups
            .into_iter()
            .find(|(key, _)| Some(key.as_str()) == selected)
        {
            body = body.child(
                h_flex()
                    .min_w_0()
                    .items_center()
                    .gap_2()
                    .child(presets::render_icon(&choices[0], rems(1.5)))
                    .child(
                        div()
                            .truncate()
                            .font_medium()
                            .child(brand_label(&choices[0])),
                    ),
            );
            body = body.child(
                div()
                    .grid()
                    .w_full()
                    .gap_2()
                    .grid_cols(if choices.len() > 4 && width >= px(480.) {
                        2
                    } else {
                        1
                    })
                    .children(choices.into_iter().map(|entry| {
                        let route = &entry.preset.routes[0];
                        let login = presets::authentication(&entry)
                            != sailry_protocol::Authentication::ApiKey;
                        let access = Access::of(
                            presets::kind(&entry, route),
                            login,
                            presets::custom_endpoint(&entry, route),
                        );
                        let label: SharedString = presets::label(&entry.preset).into();
                        surface(
                            Button::new(SharedString::from(format!(
                                "provider-method-{}",
                                entry.preset.id
                            ))),
                            cx,
                        )
                        .h(px(56.))
                        .px_3()
                        .accessibility_label(label.clone())
                        .child(
                            h_flex()
                                .size_full()
                                .min_w_0()
                                .items_center()
                                .gap_3()
                                .child(icon_tile(px(32.), access.icon().size_4(), cx))
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .items_start()
                                        .gap_0p5()
                                        .child(div().max_w_full().truncate().child(label))
                                        .child(
                                            div()
                                                .max_w_full()
                                                .truncate()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(tr(access.key())),
                                        ),
                                )
                                .child(
                                    Icon::new(IconName::ChevronRight)
                                        .size_4()
                                        .text_color(cx.theme().muted_foreground),
                                ),
                        )
                        .on_click(cx.listener(
                            move |editor, _, window, cx| {
                                editor.choose_entry(entry.clone(), window, cx)
                            },
                        ))
                    })),
            );
        }
        body.vertical_scrollbar(&self.scroll).into_any_element()
    }
}

#[cfg(test)]
mod tests;
