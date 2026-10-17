use gpui_kit::{
    component::{
        button::{Button, ButtonCustomVariant, ButtonVariants},
        *,
    },
    prelude::FluentBuilder as _,
    *,
};

pub(crate) const ICON_SIZE: Rems = rems(1.);

/// Expanded activity follows the header's native icon slot, without a card fill.
pub(crate) fn body(content: impl IntoElement, cx: &App) -> Div {
    let half_icon = ICON_SIZE.to_pixels(cx.theme().font_size) / 2.;
    let half_border = px(0.5);
    div()
        .min_w_0()
        .self_stretch()
        .ml(half_icon - half_border)
        .pl(half_icon + rems(0.5).to_pixels(cx.theme().font_size) - half_border)
        .border_l_1()
        .border_color(cx.theme().border)
        .child(content)
}

/// Activity details retain semantic failure ink and accessible diagnostics.
#[derive(Clone, Default)]
pub(crate) struct Detail {
    pub text: SharedString,
    /// Paths and commands read in the mono face.
    pub mono: bool,
    /// Model metadata remains subdued when the action is hovered.
    pub secondary: bool,
    pub state: Option<SharedString>,
    /// Quiet terminal outcomes retain their label in the status icon's tooltip.
    pub state_icon: Option<IconName>,
    pub failed: bool,
    pub running: bool,
    pub rotate_icon: bool,
    pub pending: bool,
    pub hide_caret: bool,
    /// Prose labels wrap and determine the row's height.
    pub wrap: bool,
}

impl Detail {
    pub fn plain(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    pub fn mono(text: impl Into<SharedString>) -> Self {
        Self {
            mono: true,
            ..Self::plain(text)
        }
    }

    pub fn state(mut self, state: Option<SharedString>, failed: bool) -> Self {
        self.state = state;
        self.failed = failed;
        self
    }

    fn accessible(&self, label: &SharedString) -> SharedString {
        [
            Some(label.as_ref()),
            Some(self.text.as_ref()),
            self.state.as_deref(),
        ]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .into()
    }
}

impl From<SharedString> for Detail {
    fn from(text: SharedString) -> Self {
        Self::plain(text)
    }
}

/// An inline activity trigger; detail stays adjacent to its action, not at the row edge.
pub(crate) fn trigger(
    id: String,
    icon: impl Into<Icon>,
    label: SharedString,
    detail: impl Into<Detail>,
    open: bool,
    cx: &App,
) -> Button {
    let detail = detail.into();
    let icon = glyph(icon.into(), &detail, cx);
    trigger_with_icons(id, icon, label, detail, open, cx)
}

/// The same activity trigger with a composed set of leading icons.
pub(crate) fn trigger_with_icons(
    id: String,
    icons: impl IntoElement,
    label: SharedString,
    detail: impl Into<Detail>,
    open: bool,
    cx: &App,
) -> Button {
    let detail = detail.into();
    let group = SharedString::from(id.clone());
    let summary = format!("{id}-summary");
    let state_color = tone(&detail, cx);
    Button::new(id.clone())
        .group(group.clone())
        .debug_selector(move || id.clone())
        .custom(ButtonCustomVariant::new(cx).foreground(state_color))
        .small()
        .px_0()
        .justify_start()
        .self_start()
        .min_w_0()
        .max_w_full()
        .when(detail.wrap, |button| button.w_full().h_auto().min_h_6())
        .accessibility_label(detail.accessible(&label))
        .child(row(
            summary,
            icons.into_any_element(),
            label,
            detail,
            open,
            group,
            cx,
        ))
}

/// Activity summaries retain status without offering an output disclosure.
pub(crate) fn summary(
    id: String,
    icon: impl Into<Icon>,
    label: SharedString,
    mut detail: Detail,
    cx: &App,
) -> impl IntoElement {
    detail.hide_caret = true;
    let icon = glyph(icon.into(), &detail, cx);
    let content_id = format!("{id}-summary");
    let group = SharedString::from(id.clone());
    div()
        .id(id.clone())
        .group(group.clone())
        .role(Role::Label)
        .aria_label(detail.accessible(&label))
        .debug_selector(move || id.clone())
        .flex()
        .items_center()
        .when(detail.wrap, |summary| summary.w_full().min_h_6())
        .when(!detail.wrap, |summary| summary.h_6())
        .min_w_0()
        .max_w_full()
        .self_start()
        .child(row(content_id, icon, label, detail, false, group, cx))
}

fn tone(detail: &Detail, cx: &App) -> Hsla {
    if detail.failed {
        cx.theme().danger
    } else if detail.pending {
        cx.theme().warning
    } else if detail.running && !detail.pending {
        cx.theme().muted_foreground.mix(cx.theme().foreground, 0.35)
    } else {
        cx.theme().muted_foreground
    }
}

fn row(
    id: String,
    icons: AnyElement,
    label: SharedString,
    detail: Detail,
    open: bool,
    group: SharedString,
    cx: &App,
) -> impl IntoElement {
    let state_color = tone(&detail, cx);
    let status_color = if detail.failed {
        cx.theme().danger
    } else if detail.pending {
        cx.theme().warning
    } else {
        state_color
    };
    let selector = id.clone();
    h_flex()
        .id(id.clone())
        .debug_selector(move || selector.clone())
        .min_w_0()
        .when(detail.wrap, |row| row.w_full().items_start())
        .gap_2()
        .text_size(cx.theme().typography_tokens().sm.size)
        .text_color(state_color)
        .group_hover(group.clone(), |style| {
            style.text_color(if detail.failed || detail.pending {
                state_color
            } else {
                cx.theme().foreground
            })
        })
        .child(
            div()
                .debug_selector({
                    let selector = format!("{id}-icon");
                    move || selector.clone()
                })
                .flex_shrink_0()
                .when(detail.wrap, |icon| icon.pt_0p5())
                .child(icons),
        )
        // Running and settled captions share the same typography and spacing.
        .child(
            div()
                .debug_selector({
                    let selector = format!("{id}-label");
                    move || selector.clone()
                })
                .when(detail.wrap, |label| {
                    label.flex_1().min_w_0().whitespace_normal()
                })
                .when(!detail.wrap && detail.secondary, |label| {
                    label.min_w_0().truncate()
                })
                .when(!detail.wrap && !detail.secondary, |label| {
                    label.flex_shrink_0()
                })
                .child(if detail.running && !detail.pending && !detail.failed {
                    let selector = format!("{id}-shimmer");
                    div()
                        .id(selector.clone())
                        .test_support()
                        .debug_selector(move || selector.clone())
                        .child(
                            gpui_kit::component::shimmer::ShimmerText::new(label)
                                .id(format!("{id}-label-shimmer")),
                        )
                        .into_any_element()
                } else {
                    div().child(label).into_any_element()
                }),
        )
        .when(!detail.text.is_empty(), |row| {
            row.child(
                div()
                    .debug_selector({
                        let selector = format!("{id}-detail");
                        move || selector.clone()
                    })
                    .min_w_0()
                    .text_ellipsis()
                    .when(detail.secondary && !detail.failed, |text| {
                        text.text_color(cx.theme().muted_foreground.opacity(0.65))
                    })
                    .when(detail.mono, |text| {
                        text.max_w(px(480.))
                            .font_family(cx.theme().mono_font_family.clone())
                    })
                    .child(div().child(detail.text.clone())),
            )
        })
        .when_some(
            detail
                .state
                .clone()
                .filter(|_| !detail.pending && !detail.failed),
            |row, state| {
                row.child(if let Some(icon) = detail.state_icon {
                    div()
                        .id(format!("{id}-state"))
                        .debug_selector({
                            let selector = format!("{id}-state");
                            move || selector.clone()
                        })
                        .flex_shrink_0()
                        .text_color(status_color)
                        .tooltip(move |window, cx| {
                            tooltip::Tooltip::new(state.clone()).build(window, cx)
                        })
                        .child(Icon::new(icon).size_4())
                        .into_any_element()
                } else {
                    div()
                        .flex_shrink_0()
                        .text_color(status_color)
                        .child(state)
                        .into_any_element()
                })
            },
        )
        .when(!detail.hide_caret, |row| {
            row.child(caret(open, group).when(detail.wrap, |caret| caret.pt_0p5()))
        })
}

fn glyph(icon: Icon, detail: &Detail, cx: &App) -> AnyElement {
    if detail.pending {
        Icon::empty()
            .path("icons/reicon/help-circle.svg")
            .text_color(if detail.failed {
                cx.theme().danger
            } else {
                cx.theme().warning
            })
            .size(ICON_SIZE)
            .into_any_element()
    } else {
        crate::ui::identicon::animated(
            icon.size(ICON_SIZE)
                .flex_shrink_0()
                .when(detail.failed, |icon| icon.text_color(cx.theme().danger)),
            detail.rotate_icon,
        )
    }
}

/// Preserve the hit area and inline alignment while keeping idle rows quiet.
pub(crate) fn caret(open: bool, group: SharedString) -> Div {
    div()
        .flex_shrink_0()
        .opacity(0.)
        .group_hover(group, |style| style.opacity(1.))
        .child(
            Icon::new(if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .size_3(),
        )
}

#[cfg(test)]
mod tests;
