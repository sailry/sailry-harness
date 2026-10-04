//! Shared open-document navigation for conversations, files, and Git.
//!
//! Kit b79f4ce's styled TabBar forces either square borders or a primary/background
//! indicator. Its base Tabs/Tab retain collection semantics without that styling.
use gpui_kit::{
    base::{Tab, Tabs},
    component::{
        button::{Button, ButtonCustomVariant, ButtonVariants},
        menu::{DropdownMenu, PopupMenuItem},
        *,
    },
    prelude::FluentBuilder as _,
    *,
};

pub(crate) fn item(
    id: String,
    label: SharedString,
    selected: bool,
    prefix: Option<AnyElement>,
    close: impl Into<Option<Button>>,
    cx: &App,
) -> Tab {
    let group = SharedString::from(id.clone());
    let icon_selector = format!("{id}-icon");
    let label_selector = format!("{id}-label");
    let foreground = if selected {
        cx.theme().tab_active_foreground
    } else {
        cx.theme().tab_foreground
    };
    Tab::new(id.clone())
        .group(group.clone())
        .debug_selector(move || id.clone())
        .accessibility_label(label.clone())
        .selected(selected)
        .h_7()
        .px_2()
        .gap_2()
        .min_w_0()
        .max_w(px(240.))
        .flex_shrink_0()
        .rounded(cx.theme().radius)
        .text_sm()
        .bg(if selected {
            cx.theme().tab_active
        } else {
            cx.theme().transparent
        })
        .text_color(foreground)
        .hover(|style| {
            style.text_color(if selected {
                foreground
            } else {
                cx.theme().foreground
            })
        })
        .when_some(prefix, |tab, prefix| {
            tab.child(
                div()
                    .flex_shrink_0()
                    .debug_selector(move || icon_selector.clone())
                    .child(prefix),
            )
        })
        .child(
            div()
                .min_w_0()
                .debug_selector(move || label_selector.clone())
                .truncate()
                .child(label),
        )
        .when_some(close.into(), |tab, close| {
            tab.child(
                close
                    .when(selected, |close| {
                        close.custom(
                            ButtonCustomVariant::new(cx)
                                .foreground(foreground)
                                .hover(foreground.opacity(cx.theme().accent.a))
                                .active(foreground.opacity(cx.theme().secondary_active.a)),
                        )
                    })
                    .opacity(0.)
                    .group_hover(group, |style| style.opacity(1.))
                    .focus(|style| style.opacity(1.)),
            )
        })
}

pub(crate) fn strip(
    id: impl Into<SharedString>,
    scroll: &ScrollHandle,
    items: Vec<(SharedString, AnyElement)>,
    selected: Option<usize>,
    on_select: impl Fn(&usize, &mut Window, &mut App) + 'static,
    _cx: &App,
) -> AnyElement {
    let id = id.into();
    let labels = items
        .iter()
        .map(|(label, _)| label.clone())
        .collect::<Vec<_>>();
    let count = items.len();
    let on_select = std::rc::Rc::new(on_select);
    Tabs::new(id.clone())
        .flex()
        .items_center()
        .h_8()
        .w_full()
        .min_w_0()
        .flex_1()
        .child(
            h_flex()
                .id(format!("{id}-scroll"))
                .flex_1()
                .min_w_0()
                .overflow_x_scroll()
                .track_scroll(scroll)
                .gap_1()
                .children(
                    items
                        .into_iter()
                        .map(|(_, item)| h_flex().flex_shrink_0().child(item)),
                ),
        )
        .when(count > 4, |bar| {
            bar.child(
                Button::new(format!("{id}-more"))
                    .ghost()
                    .xsmall()
                    .icon(IconName::ChevronDown)
                    .accessibility_label(crate::tr("navigation_tabs"))
                    .dropdown_menu(move |mut menu, _, _| {
                        for (index, label) in labels.iter().enumerate() {
                            let on_select = on_select.clone();
                            menu = menu.item(
                                PopupMenuItem::new(label.clone())
                                    .checked(selected == Some(index))
                                    .on_click(move |_, window, cx| on_select(&index, window, cx)),
                            );
                        }
                        menu
                    }),
            )
        })
        .into_any_element()
}
