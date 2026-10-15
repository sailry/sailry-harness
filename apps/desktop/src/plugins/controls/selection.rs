//! Kit's script Button omits selected state and custom-content accessible names.
//! Keep selectable rows and named connection cards on the native Kit button.
use gpui_kit::base::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{
        ActiveTheme as _, Selectable as _,
        button::{Button, ButtonVariants as _},
        list::ListItem,
    },
    *,
};
use gpui_shell::{HostModule, HostValue};
use serde_json::{Value, json};
use tokio::sync::mpsc::Sender;

pub(super) fn extend(module: HostModule, events: Sender<Value>) -> HostModule {
    module.component("SelectableRow", move |mut args, _, cx| {
        let id = args.id().to_owned();
        let selected = args.props().get("selected").and_then(HostValue::as_bool) == Some(true);
        let disabled = args.props().get("disabled").and_then(HostValue::as_bool) == Some(true);
        let stripe = args.props().get("stripe").and_then(HostValue::as_bool);
        let label = args
            .props()
            .get("label")
            .and_then(HostValue::as_str)
            .map(str::to_owned);
        let variant = args.props().get("variant").and_then(HostValue::as_str);
        let events = events.clone();
        if variant == Some("list") {
            return ListItem::new(id.clone())
                .selected(selected)
                .disabled(disabled)
                .rounded_none()
                .px_3()
                .h_8()
                .gap_3()
                .text_sm()
                .border_b_1()
                .border_color(cx.theme().border)
                .text_color(cx.theme().muted_foreground)
                .when_some(stripe, |row, stripe| {
                    row.bg(if stripe {
                        cx.theme().tokens.table_even
                    } else {
                        cx.theme().tokens.table
                    })
                })
                .debug_selector({
                    let id = id.clone();
                    move || id.clone()
                })
                .children(args.take_children())
                .on_click(move |event, _, _| {
                    let _ = events.try_send(json!({"id":id,"click_count":event.click_count()}));
                })
                .into_any_element();
        }
        let button = Button::new(id.clone());
        let button = if variant == Some("card") {
            button.outline().p_3()
        } else {
            button.ghost().px_2()
        };
        button
            .w_full()
            .justify_start()
            .h_auto()
            .selected(selected)
            .disabled(disabled)
            .when_some(label, |button, label| button.accessibility_label(label))
            .debug_selector({
                let id = id.clone();
                move || id.clone()
            })
            .children(args.take_children())
            .on_click(move |_, _, _| {
                let _ = events.try_send(json!({"id":id}));
            })
            .into_any_element()
    })
}
