//! Declarative tree adornments reuse Kit typography, badges and accessible checkboxes.
use super::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::list::ListItem;
use gpui_kit::prelude::FluentBuilder;
use std::collections::BTreeSet;

pub(super) fn last_children(items: &[Item]) -> BTreeSet<String> {
    let mut last = BTreeSet::new();
    fn collect(items: &[Item], last: &mut BTreeSet<String>) {
        for item in items {
            if let Some(child) = item.children.last() {
                last.insert(child.id.clone());
            }
            collect(&item.children, last);
        }
    }
    collect(items, &mut last);
    last
}

// Kit b79f4ce's script Tree fixes every row to a file/folder glyph and cannot
// render disclosure or hierarchy guides. Reuse its native ListItem presentation
// under the same retained TreeState and event owner for branch-style resources.
pub(super) fn branch(
    index: usize,
    entry: &TreeEntry,
    selected: bool,
    last: bool,
    cx: &App,
) -> ListItem {
    ListItem::new(("resource-branch", index))
        .w_full()
        .min_w_0()
        .selected(selected)
        .disabled(entry.is_disabled())
        .h(px(27.))
        .mb(px(1.))
        .py_0()
        .text_sm()
        .pl(px(8. + 20. * entry.depth() as f32))
        .rounded(cx.theme().radius)
        .child(
            h_flex()
                .gap_2()
                .min_w_0()
                .relative()
                .h(px(27.))
                .when(entry.depth() > 0, |row| {
                    row.child(
                        div()
                            .absolute()
                            .left(px(-14.))
                            .top_0()
                            .h(if last { px(14.) } else { px(28.) })
                            .border_l_1()
                            .border_dashed()
                            .border_color(cx.theme().muted_foreground.opacity(0.65)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(-14.))
                            .top(px(14.))
                            .w_2()
                            .border_t_1()
                            .border_dashed()
                            .border_color(cx.theme().muted_foreground.opacity(0.65)),
                    )
                })
                .when(entry.is_folder(), |row| {
                    row.child(
                        Icon::new(if entry.is_expanded() {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .size_3(),
                    )
                })
                .child(div().truncate().child(entry.item().label.clone())),
        )
}

#[derive(Clone, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub(super) struct Decoration {
    pub selector: Option<String>,
    pub detail: Option<String>,
    pub icon_tone: Option<String>,
    pub icon_size: Option<f32>,
    #[serde(default)]
    pub badges: Vec<Badge>,
    pub check: Option<Check>,
}
#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Badge {
    text: String,
    tone: Option<String>,
}
#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Check {
    id: String,
    label: String,
    checked: bool,
    #[serde(default)]
    mixed: bool,
    #[serde(default)]
    disabled: bool,
}
fn color(tone: Option<&str>, cx: &App) -> Hsla {
    match tone {
        Some("success") => cx.theme().success,
        Some("danger") => cx.theme().danger,
        Some("warning") => cx.theme().warning,
        Some("info") => cx.theme().info,
        Some("muted") => cx.theme().muted_foreground,
        _ => cx.theme().foreground,
    }
}

pub(super) fn content(
    item: &Item,
    glyph: &str,
    events: Entity<Events>,
    tree: String,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let decoration = item.decoration.clone().unwrap_or_default();
    let glyph = icon(glyph)
        .with_size(px(decoration.icon_size.unwrap_or(16.)))
        .text_color(color(decoration.icon_tone.as_deref(), cx))
        .flex_shrink_0();
    h_flex()
        .w_full()
        .min_w_0()
        .gap_2()
        .child(glyph)
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .gap_2()
                .child(
                    div()
                        .min_w_0()
                        .max_w_full()
                        .truncate()
                        .child(item.label.clone()),
                )
                .children(decoration.detail.map(|detail| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(cx.theme().muted_foreground)
                        .child(detail)
                })),
        )
        .children(decoration.badges.into_iter().map(|badge| {
            div()
                .text_xs()
                .flex_shrink_0()
                .text_color(color(badge.tone.as_deref(), cx))
                .child(badge.text)
        }))
        .children(decoration.check.map(|check| {
            let path = item.id.clone();
            let check_id = check.id.clone();
            let change = move |_: &bool, _: &mut Window, cx: &mut App| {
                cx.stop_propagation();
                events
                    .read(cx)
                    .send(json!({"tree":tree,"id":path,"kind":"check"}));
            };
            div()
                .id(SharedString::from(format!("check-region-{check_id}")))
                .flex_shrink_0()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(checkbox(check, window, cx, change))
        }))
        .into_any_element()
}

// Component Checkbox exposes bool only in b79f4ce; Base Checkbox supplies mixed semantics.
fn checkbox(
    check: Check,
    window: &mut Window,
    cx: &mut App,
    on_click: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id = check.id;
    if !check.mixed {
        return Checkbox::new(SharedString::from(id.clone()))
            .small()
            .checked(check.checked)
            .disabled(check.disabled)
            .accessibility_label(check.label)
            .debug_selector(move || id.clone())
            .on_click(on_click)
            .into_any_element();
    }
    let focus = window
        .use_keyed_state(
            ElementId::from(SharedString::from(id.clone())),
            cx,
            |_, cx| cx.focus_handle(),
        )
        .read(cx)
        .clone();
    let color = cx
        .theme()
        .primary
        .opacity(if check.disabled { 0.5 } else { 1. });
    gpui_kit::base::Checkbox::new(SharedString::from(id.clone()))
        .indeterminate(true)
        .disabled(check.disabled)
        .track_focus(&focus)
        .accessibility_label(check.label)
        .debug_selector(move || id.clone())
        .rounded(cx.theme().radius * 0.5)
        .when(focus.is_focused(window), |checkbox| {
            checkbox.focus_ring_style(window, cx)
        })
        .child(
            div()
                .size_3p5()
                .flex()
                .items_center()
                .justify_center()
                .border_1()
                .rounded(cx.theme().radius.min(px(4.)))
                .bg(color)
                .border_color(color)
                .text_color(cx.theme().primary_foreground)
                .child(Icon::new(IconName::Minus).size_3()),
        )
        .on_change(move |_, _, window, cx| {
            window.prevent_default();
            on_click(&true, window, cx);
        })
        .into_any_element()
}
