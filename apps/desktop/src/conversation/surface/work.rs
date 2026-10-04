//! Bezel's Work zone composed with Kit disclosure controls.
//! Adapted from Bezel 4a7505ab, gallery/patterns/transcript.rs (MIT).
//! See third_party_licenses/bezel.md. Expansion remains owned by each transcript.
use gpui_kit::{
    component::{
        ActiveTheme, Icon, IconName, Sizable as _,
        button::{Button, ButtonCustomVariant, ButtonVariants as _},
        collapsible::Collapsible,
        h_flex, v_flex,
    },
    prelude::FluentBuilder as _,
    *,
};

/// The existing turn status is also the process disclosure's trigger.
pub(crate) fn work_trigger(
    id: String,
    label: SharedString,
    status: impl IntoElement,
    open: bool,
    cx: &App,
) -> Button {
    let chevron = format!("{id}-chevron");
    let group = SharedString::from(id.clone());
    Button::new(id.clone())
        .group(group.clone())
        .debug_selector(move || id.clone())
        .custom(ButtonCustomVariant::new(cx).foreground(cx.theme().muted_foreground))
        .small()
        .px_0()
        .w_full()
        .min_w_0()
        .accessibility_label(label)
        .toggled(open)
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .gap_2()
                .text_sm()
                .child(div().flex_1().min_w_0().child(status))
                .child(
                    div()
                        .debug_selector(move || chevron.clone())
                        .flex_shrink_0()
                        .opacity(0.)
                        .group_hover(group, |style| style.opacity(1.))
                        .child(
                            Icon::new(if open {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .size_4(),
                        ),
                ),
        )
}

/// Only process rows collapse; the final answer remains outside the region.
pub(crate) fn work(
    mut rows: Vec<AnyElement>,
    answer_from: usize,
    open: bool,
    cx: &App,
) -> Vec<AnyElement> {
    if answer_from == 0 {
        return rows;
    }
    let answer = rows.split_off(answer_from);
    let process = Collapsible::new()
        .open(open)
        .w_full()
        .min_w_0()
        .when(!open, |region| region.hidden())
        .content(
            v_flex()
                .w_full()
                .min_w_0()
                .gap(cx.theme().spacing_tokens().lg)
                .children(rows),
        )
        .into_any_element();
    std::iter::once(process).chain(answer).collect()
}
