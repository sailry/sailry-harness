//! Small Kit adapters for script gaps: icon actions, boolean controls, and document tabs.
//! The pinned TabBar lacks prefix/close slots; reuse Sailry's existing base Tabs composition.
mod appearance;
mod menu;
mod models;
mod segments;
mod select;
mod selection;
mod shortcuts;
pub(super) mod tabs;

use super::host::sdk::values::{decode, encode};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        checkbox::Checkbox,
        switch::Switch,
        *,
    },
    *,
};
use gpui_shell::{HostError, HostModule, HostValue};
use sailry_link::CancellationToken;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc};
fn icon(value: &str) -> Icon {
    Icon::empty().path(if value.contains(':') || value.contains('/') {
        value.to_owned()
    } else {
        format!("icons/{value}.svg")
    })
}

pub(super) fn module(
    module: HostModule,
    stop: CancellationToken,
    tabs: tabs::Controls,
    cx: &mut App,
) -> HostModule {
    let (controls, control_events) = tokio::sync::mpsc::channel::<Value>(32);
    let control_events = Arc::new(tokio::sync::Mutex::new(control_events));
    let buttons = controls.clone();
    let actions = controls.clone();
    let control_stop = stop.clone();
    let declarations = format!(
        "{}\n{}",
        module.declared().unwrap_or_default(),
        include_str!("controls.d.ts")
    );
    let module = appearance::extend(module, controls.clone(), stop.clone());
    let module = shortcuts::extend(module, stop.clone(), cx);
    let module = selection::extend(module, controls.clone());
    let module = menu::extend(module, controls.clone());
    let module = models::extend(module, controls.clone());
    let module = segments::extend(module, controls.clone());
    let module = tabs.extend(module);
    select::extend(module, controls.clone()).component("SelectableText", move |args, _, _| {
        // Kit b79f4ce's script catalog has Text, but no selectable TextView.
        let text = args.props().get("text").and_then(HostValue::as_str).unwrap_or_default().to_owned();
        gpui_kit::base::text::TextView::markdown(args.id().to_owned(), text).scrollable(true).into_any_element()
    }).component("ActionScope", move |mut args, _, _| {
        // Keep the standard panel-close action available to script containers.
        let mut region = div().size_full().min_w_0().min_h_0();
        if let Some(id) = args.props().get("close").and_then(HostValue::as_str).map(str::to_owned) {
            let sender = actions.clone();
            region = region.key_context("ResourcePanel").on_action(move |_: &crate::shell::shortcuts::CloseFocused, _, _| {
                let _ = sender.try_send(json!({"id":id}));
            });
        }
        region.children(args.take_children()).into_any_element()
    }).component("IconButton",move |args,_,cx| {
        let props = args.props();
        let id = args.id().to_owned();
        let label = props.get("label").and_then(HostValue::as_str).unwrap_or_default().to_owned();
        let glyph = props.get("icon").and_then(HostValue::as_str).unwrap_or_default();
        let disabled = props.get("disabled").and_then(HostValue::as_bool).unwrap_or(false);
        let selected = props.get("selected").and_then(HostValue::as_bool).unwrap_or(false);
        let tone = props.get("tone").and_then(HostValue::as_str);
        let primary = props.get("variant").and_then(HostValue::as_str) == Some("primary");
        let circular = props.get("circular").and_then(HostValue::as_bool) == Some(true);
        let medium = props.get("size").and_then(HostValue::as_str) == Some("medium");
        let full_width = props.get("full_width").and_then(HostValue::as_bool) == Some(true);
        // Script Button has no native icon slot; retain its icon-label ordering.
        let show_label = props.get("show_label").and_then(HostValue::as_bool) == Some(true);
        let dropdown_caret = props.get("dropdown_caret").and_then(HostValue::as_bool) == Some(true);
        let sender = buttons.clone();
        let button = Button::new(SharedString::from(id.clone())).debug_selector({let id=id.clone(); move || id.clone()})
            .map(|button| if primary { button.primary() } else { button.ghost() })
            .map(|button| if medium { button } else { button.small() })
            .when(full_width, |button| button.w_full())
            .when(circular, |button| button.rounded_full())
            .icon(icon(glyph))
            .when(show_label, |button| button.label(label.clone()))
            .dropdown_caret(dropdown_caret)
            .selected(selected)
            .when(!show_label, |button| button.tooltip(label.clone()))
            .accessibility_label(label).disabled(disabled)
            .when(tone == Some("success"), |button| button.text_color(cx.theme().success))
            .when(tone == Some("muted"), |button| button.text_color(cx.theme().muted_foreground))
            .on_click(move |_,_,_| {let _=sender.try_send(json!({"id":id}));});
        button.into_any_element()
    }).component("Toggle",move |args,_,_| {
        let props = args.props();
        let id = args.id().to_owned();
        let label = props.get("label").and_then(HostValue::as_str).unwrap_or_default().to_owned();
        let checked = props.get("checked").and_then(HostValue::as_bool).unwrap_or(false);
        let disabled = props.get("disabled").and_then(HostValue::as_bool).unwrap_or(false);
        let sender = controls.clone();
        // Kit b79f4ce's script Checkbox has no independent accessible-name API.
        // Reuse this boolean adapter for label-free native controls.
        if props.get("variant").and_then(HostValue::as_str) == Some("checkbox") {
            return div().debug_selector({let id=id.clone(); move || id.clone()}).child(
                Checkbox::new(SharedString::from(id.clone())).checked(checked).disabled(disabled).accessibility_label(label)
                    .on_click(move |value,_,_| {let _=sender.try_send(json!({"id":id,"value":value}));})
            ).into_any_element();
        }
        if props.get("variant").and_then(HostValue::as_str) == Some("button") {
            // Kit b79f4ce omits Button.selected in scripts; keep its existing button styling.
            let text = props.get("text").and_then(HostValue::as_str).unwrap_or(&label).to_owned();
            return div().debug_selector({let id=id.clone(); move || id.clone()}).child(
                Button::new(SharedString::from(id.clone())).ghost().small().label(text)
                    .tooltip(label.clone()).accessibility_label(label).selected(checked).toggled(checked).disabled(disabled)
                    .on_click(move |_,_,_| {let _=sender.try_send(json!({"id":id,"value":!checked}));})
            ).into_any_element();
        }
        div().debug_selector({let id=id.clone(); move || id.clone()}).child(
            Switch::new(SharedString::from(id.clone())).checked(checked).disabled(disabled).accessibility_label(label)
                .on_click(move |value,_,_| {let _=sender.try_send(json!({"id":id,"value":value}));})
        ).into_any_element()
    }).async_function("nextControlEvent",move |_| {
        let receiver=control_events.clone();let stop=control_stop.clone();
        Ok(async move {tokio::select! {biased; _=stop.cancelled()=>Err(HostError::new("plugin view is closed")), event=async{receiver.lock().await.recv().await}=>event.ok_or_else(||HostError::new("controls are closed")).and_then(encode)}})
    }).declarations(declarations)
}
