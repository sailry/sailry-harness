//! Project and subagent editors share the same Kit identity chooser.
use super::{color, project};
use crate::tr;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    popover::Popover,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sailry_protocol::projects::{Appearance, COLORS, ICONS};

pub(crate) struct Selection<T> {
    pub selected: Appearance,
    pub read: fn(&T) -> Appearance,
    pub write: fn(&mut T, Appearance),
}

pub(crate) fn picker<T: 'static>(
    id: &str,
    owner: WeakEntity<T>,
    selection: Selection<T>,
    disabled: bool,
    label: SharedString,
    cx: &App,
) -> Popover {
    let Selection {
        selected,
        read,
        write,
    } = selection;
    let id = id.to_owned();
    let trigger = format!("{id}-appearance");
    Popover::new(SharedString::from(format!("{id}-appearance")))
        .trigger(
            Button::new(SharedString::from(format!("{id}-identity")))
                .text()
                .child(project(&selected, cx))
                .disabled(disabled)
                .accessibility_label(label)
                .debug_selector(move || trigger.clone()),
        )
        .content(move |_, _, cx| {
            let Some(owner) = owner.upgrade() else {
                return div().into_any_element();
            };
            let selected = read(owner.read(cx));
            let colors = format!("{id}-colors");
            v_flex()
                .w_64()
                .gap_3()
                .p_3()
                .child(
                    h_flex()
                        .debug_selector(move || colors.clone())
                        .w_full()
                        .px_2()
                        .gap_1()
                        .children(COLORS.iter().map(|&name| {
                            let owner = owner.clone();
                            let selector = format!("{id}-color-{name}");
                            let check = format!("{id}-color-check");
                            // Named identities use Kit swatches without the unrestricted HSLA editor.
                            gpui_kit::base::ColorSwatch::new(name, color(name, cx))
                                .size_4()
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .bg(color(name, cx))
                                .border_1()
                                .border_color(if selected.color == name {
                                    cx.theme().foreground
                                } else {
                                    cx.theme().border
                                })
                                .selected(selected.color == name)
                                .accessibility_label(tr(&format!("project_color_{name}")))
                                .debug_selector(move || selector.clone())
                                .when(selected.color == name, |swatch| {
                                    swatch.child(
                                        h_flex()
                                            .debug_selector(move || check.clone())
                                            .size_2p5()
                                            .child(
                                                Icon::new(IconName::Check)
                                                    .size_2p5()
                                                    .text_color(cx.theme().background),
                                            ),
                                    )
                                })
                                .on_click(move |_, _, _, cx| {
                                    owner.update(cx, |owner, cx| {
                                        let mut value = read(owner);
                                        value.color = name.into();
                                        write(owner, value);
                                        cx.notify();
                                    })
                                })
                        })),
                )
                .children(ICONS.chunks(6).enumerate().map(|(row, icons)| {
                    let row_selector = format!("{id}-icon-row-{row}");
                    h_flex()
                        .debug_selector(move || row_selector.clone())
                        .w_full()
                        .gap_2()
                        .children(icons.iter().enumerate().map(|(column, &icon)| {
                            let owner = owner.clone();
                            let selector = format!("{id}-icon-{icon}");
                            Button::new((
                                SharedString::from(format!("{id}-icon")),
                                row * 6 + column,
                            ))
                            .ghost()
                            .flex_1()
                            .h_8()
                            .p_0()
                            .selected(selected.icon == icon)
                            .child(project(
                                &Appearance {
                                    icon: icon.into(),
                                    color: selected.color.clone(),
                                },
                                cx,
                            ))
                            .accessibility_label(tr(&format!("project_icon_{icon}")))
                            .debug_selector(move || selector.clone())
                            .on_click(move |_, _, cx| {
                                owner.update(cx, |owner, cx| {
                                    let mut value = read(owner);
                                    value.icon = icon.into();
                                    write(owner, value);
                                    cx.notify();
                                })
                            })
                        }))
                }))
                .into_any_element()
        })
}
