//! Script Command cannot expose native autofocus or its empty-content slot.
//! Keep those in the existing picker lifecycle; packages own rows and choices.
use super::*;
use gpui_kit::component::command::{Command, CommandItem, CommandState};
use gpui_kit::prelude::FluentBuilder as _;

pub(super) fn popup(
    id: String,
    props: Props,
    state: Entity<CommandState>,
    events: Entity<Events>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    if state.read(cx).is_loading() != props.loading {
        state.update(cx, |state, cx| state.set_loading(props.loading, window, cx));
    }
    let focus = state.read(cx).focus_handle(cx);
    let width = px(480.).min(window.viewport_size().width - px(40.));
    let rows = props.items.clone();
    let selected_id = id.clone();
    let selected_events = events.clone();
    let empty = props.empty;
    let command = Command::new(&state)
        .bordered(false)
        .text_sm()
        .line_height(relative(1.25))
        .max_h(crate::command_picker::body_height(window))
        .placeholder(props.title)
        .items(props.items.into_iter().map(item))
        .empty(move |_, _, cx| {
            div()
                .debug_selector(|| "plugin-picker-empty".into())
                .p_3()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(empty.clone())
        })
        .on_confirm(move |index, _, cx| {
            if let Some(row) = rows.get(index.row).filter(|row| !row.disabled) {
                selected_events.read(cx).send(json!({
                    "picker": selected_id, "kind": "select", "id": row.id, "value": row.value
                }));
            }
        });
    base::Dialog::new(cx)
        .focus_handle(focus)
        .on_ok(|_, _, _| false)
        .on_close({
            let id = id.clone();
            move |_, _, cx| events.read(cx).send(json!({"picker":id,"kind":"close"}))
        })
        .backdrop(div().absolute().inset_0().bg(cx.theme().overlay))
        .popup(
            div()
                .absolute()
                .left((window.viewport_size().width - width) / 2.)
                .top(window.viewport_size().height / 10.)
                .w(width)
                .child(
                    div()
                        .id(SharedString::from(id.clone()))
                        .debug_selector(move || id.clone())
                        .occlude()
                        .min_h_0()
                        .p_0()
                        .overflow_hidden()
                        .w(width)
                        .max_h(window.viewport_size().height * 0.6)
                        .bg(cx.theme().popover)
                        .text_color(cx.theme().popover_foreground)
                        .border_1()
                        .border_color(cx.theme().border)
                        .rounded(cx.theme().radius_lg)
                        .shadow_xl()
                        .child(command),
                ),
        )
        .into_any_element()
}

fn item(row: Item) -> CommandItem {
    CommandItem::new()
        .label(row.label.clone())
        .keywords([format!("{} {}", row.label, row.detail)])
        .checked(row.checked)
        .disabled(row.disabled)
        .child(move |_, cx| {
            let selector = row.id.clone();
            h_flex()
                .debug_selector(move || selector.clone())
                .w_full()
                .min_w_0()
                .gap_2()
                .children(row.icon.clone().map(|icon| {
                    Icon::empty()
                        .path(if icon.contains('/') {
                            icon
                        } else {
                            format!("icons/{icon}.svg")
                        })
                        .size_4()
                }))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .line_height(relative(1.25))
                        .child(div().truncate().child(row.label.clone()))
                        .when(!row.detail.is_empty(), |column| {
                            column.child(
                                div()
                                    .truncate()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(row.detail.clone()),
                            )
                        }),
                )
                .when(row.submenu, |item| {
                    item.child(Icon::new(IconName::ChevronRight).size_4())
                })
        })
}
