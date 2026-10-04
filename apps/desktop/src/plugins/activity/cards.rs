//! Kit card controls and small script adapters for native hover and line clamping.
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{
        button::{Button, ButtonCustomVariant, ButtonVariants},
        *,
    },
    *,
};
use gpui_shell::{HostError, HostModule, HostValue};
use sailry_link::CancellationToken;
use std::sync::Arc;

#[cfg(test)]
mod tests;

pub(super) fn module(module: HostModule, stop: CancellationToken) -> HostModule {
    let declarations = format!(
        "{}\n{}",
        module.declared().unwrap_or_default(),
        r#"
        export const CardButton:{new(id:string,props:{label:string;tooltip:string}):import('gpui-kit').Element};
        export const ComposedButton:{new(id:string,props:{label:string}):import('gpui-kit').Element};
        export function nextCardEvent():Promise<string>;
        export const ClampedText:{new(id:string,props:{text:string;lines?:number;danger?:boolean}):import('gpui-kit').Element};
        /** Kit's group hover is not exposed to scripts; children are default and hovered content. */
        export const HoverSwap:{new(id:string,props:{group:string}):import('gpui-kit').Element};
        /** The same animated indicator used by the conversation's execution phase. */
        export const ThinkingIcon:{new(id:string,props:{phase:string}):import('gpui-kit').Element};
    "#
    );
    let (cards, events) = tokio::sync::mpsc::channel(32);
    let composed = cards.clone();
    let events = Arc::new(tokio::sync::Mutex::new(events));
    module
        // The pinned script Button lacks custom variants and composed accessibility names.
        .component("CardButton", move |mut args, _, cx| {
            let id = args.id().to_owned();
            let label = string(args.props(), "label");
            let tooltip = string(args.props(), "tooltip");
            let cards = cards.clone();
            // Light cards share GroupBox's configured surface; retain the dark inset.
            let background = if cx.theme().is_dark() {cx.theme().muted} else {cx.theme().group_box};
            Button::new(SharedString::from(id.clone()))
                .group(id.clone())
                .debug_selector({let id=id.clone();move ||id.clone()})
                .accessibility_label(label).tooltip(tooltip)
                .custom(ButtonCustomVariant::new(cx)
                    .hover(background.blend(cx.theme().accent))
                    .active(background.blend(cx.theme().secondary_active)))
                .bg(background).w_full().min_w_0().h_auto()
                .flex_shrink_0().p_3().border_1().border_color(cx.theme().border)
                .rounded(cx.theme().radius_lg)
                .children(args.take_children())
                .on_click(move |_, _, _| {let _=cards.try_send(id.clone());})
                .into_any_element()
        })
        .component("ComposedButton", move |mut args, _, _| {
            let id = args.id().to_owned();
            let label = string(args.props(), "label");
            let events = composed.clone();
            Button::new(SharedString::from(id.clone()))
                .debug_selector({let id=id.clone();move ||id.clone()})
                .accessibility_label(label).ghost().w_full().h_8().px_1()
                .children(args.take_children())
                .on_click(move |_, _, _| {let _=events.try_send(id.clone());})
                .into_any_element()
        })
        .component("ClampedText", |args, _, cx| {
            let id = args.id().to_owned();
            let text = string(args.props(), "text");
            let lines = args.props().get("lines").and_then(HostValue::as_number)
                .filter(|value| value.is_finite() && value.fract()==0. && (1. ..=8.).contains(value))
                .unwrap_or(2.) as usize;
            let danger = args.props().get("danger").and_then(HostValue::as_bool)==Some(true);
            // Pinned GPUI counts soft wraps, not hard breaks; cap both without reserving height.
            div().debug_selector(move ||id.clone()).w_full().h_auto().max_h(rems(1.25 * lines as f32))
                .flex_shrink_0().line_height(rems(1.25)).text_left().text_sm().font_weight(FontWeight::NORMAL)
                .text_color(if danger {cx.theme().danger}else{cx.theme().muted_foreground})
                .whitespace_normal().line_clamp(lines).overflow_hidden().child(text)
                .into_any_element()
        })
        .component("HoverSwap", |mut args, _, _| {
            let id = args.id().to_owned();
            let group = string(args.props(), "group");
            let mut children = args.take_children().into_iter();
            let default = children.next();
            let hovered = children.next();
            let swap = hovered.is_some();
            let default_id = format!("{id}-default");
            let hovered_id = format!("{id}-hovered");
            // Native group styles keep the slot stable and avoid a separate hover state machine.
            div().debug_selector(move || id.clone()).relative().w(rems(7.)).max_w(relative(0.5)).h_4().flex_shrink_0()
                .child(div().debug_selector(move ||default_id.clone()).size_full()
                    .when(swap,|element|element.group_hover(group.clone(),|style|style.invisible()))
                    .children(default))
                .children(hovered.map(|child|div().debug_selector(move ||hovered_id.clone()).absolute().inset_0().invisible()
                    .group_hover(group,|style|style.visible()).child(child)))
                .into_any_element()
        })
        .component("ThinkingIcon", |args, _, _| {
            crate::ui::thinking::render(args.id().to_owned(), &string(args.props(), "phase"), px(20.))
        })
        .async_function("nextCardEvent", move |_| {
            let events=events.clone();let stop=stop.clone();
            Ok(async move {tokio::select! {biased;
                _=stop.cancelled()=>Err(HostError::new("plugin view is closed")),
                value=async{events.lock().await.recv().await}=>value.map(HostValue::from).ok_or_else(||HostError::new("card events are closed")),
            }})
        })
        .declarations(declarations)
}

fn string(props: &HostValue, name: &str) -> String {
    props
        .get(name)
        .and_then(HostValue::as_str)
        .unwrap_or_default()
        .to_owned()
}
