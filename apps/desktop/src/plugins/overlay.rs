//! Kit b79f4ce's script window dialogs require ShellRoot as the OS window root.
//! Embedded plugins instead compose the native Base dialog in their own view.
//! Kit retains focus trapping, keyboard dismissal, stacking and theme surfaces.
use gpui_kit::{
    base::Disableable as _,
    component::{
        ActiveTheme as _, IconName, Sizable as _, StyledExt as _,
        animation::cubic_bezier,
        button::ButtonVariants as _,
        dialog::{ANIMATION_DURATION, DialogClose, DialogContent, DialogFooter, DialogTitle},
        scroll::ScrollableElement as _,
    },
    *,
};
use gpui_shell::{HostError, HostModule, HostValue};
use std::{cell::RefCell, rc::Rc, sync::Arc};

struct State {
    open: bool,
    focus: FocusHandle,
    previous: Option<WeakFocusHandle>,
}

fn close_button(id: String, dismissable: bool) -> AnyElement {
    div()
        .absolute()
        .top(px(14.))
        .right(px(14.))
        .size_6()
        .debug_selector(move || format!("{id}-close"))
        .child(DialogClose::new().trigger(move |button| {
            button
                .small()
                .ghost()
                .icon(IconName::Close)
                .accessibility_label(crate::tr("close"))
                .tooltip(crate::tr("close"))
                .disabled(!dismissable)
        }))
        .into_any_element()
}

// Kit's window Dialog stores lazy content in Window Root, which cannot own
// eager script slots. Compose its public form parts in the existing Base host.
fn form_popup(
    id: String,
    title: String,
    width: f32,
    children: Vec<AnyElement>,
    dismissable: bool,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let mut slots = children.into_iter();
    let body = slots.next();
    let footer = slots.next();
    let width = px(width).min((window.viewport_size().width - px(48.)).max(px(0.)));
    let selector = id.clone();
    let card = div()
        .id(SharedString::from(id.clone()))
        .debug_selector(move || selector.clone())
        .occlude()
        .relative()
        .w(width)
        .max_h(window.viewport_size().height * 0.8)
        .min_h_24()
        .v_flex()
        .pt_6()
        .pb_6()
        .gap_6()
        .bg(cx.theme().tokens.background)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().surface_radius())
        .shadow_xl()
        .child(
            div()
                .v_flex()
                .flex_1()
                .overflow_hidden()
                .gap_y_2()
                .child(
                    DialogTitle::new()
                        .px_6()
                        .child(div().pr_6().pb_4().child(title)),
                )
                .child(
                    div()
                        .id("plugin-form-body")
                        .flex_1()
                        .overflow_hidden()
                        .child(
                            div()
                                .v_flex()
                                .size_full()
                                .overflow_y_scrollbar()
                                .px_6()
                                .child(DialogContent::new().pt_4().gap_4().children(body)),
                        ),
                ),
        )
        .children(footer.map(|footer| {
            div()
                .px_6()
                .child(DialogFooter::new().w_full().child(footer))
        }))
        .child(close_button(id, dismissable));
    let top = window.viewport_size().height / 10.;
    div()
        .absolute()
        .left((window.viewport_size().width - width) / 2.)
        .w(width)
        .child(component::surface::render(card, cx))
        .with_animation(
            "plugin-form-slide",
            Animation::new(*ANIMATION_DURATION).with_easing(cubic_bezier(
                1. / 3.,
                0.72,
                2. / 3.,
                1.,
            )),
            move |element, progress| element.top(top * progress),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests;

pub(super) fn module(cx: &mut App) -> HostModule {
    let state = Rc::new(RefCell::new(State {
        open: false,
        focus: cx.focus_handle(),
        previous: None,
    }));
    let (sender, receiver) = tokio::sync::mpsc::channel(8);
    let receiver = Arc::new(tokio::sync::Mutex::new(receiver));
    HostModule::new("sailry/ui")
        // Script div tooltips target ShellRoot, absent in embedded plugin views.
        // Use the application's Kit tooltip trigger without creating a button.
        .component("Tooltip", |mut args, _, _| {
            let text = args.props().get("text").and_then(HostValue::as_str).unwrap_or_default().to_owned();
            let trigger = div().id(args.id().to_owned()).size_full()
                .tooltip(move |window, cx| {
                    #[cfg(not(test))]
                    let tooltip = component::tooltip::Tooltip::new(text.clone());
                    #[cfg(test)]
                    let tooltip = component::tooltip::Tooltip::element({
                        let text = text.clone();
                        move |_, _| div().debug_selector({
                            let text = text.clone();
                            move || format!("plugin-tooltip-{text}")
                        }).child(text.clone()).into_any_element()
                    });
                    tooltip.build(window, cx)
                })
                .children(args.take_children());
            #[cfg(test)]
            let trigger = trigger.debug_selector(|| args.id().to_owned());
            trigger.into_any_element()
        })
        .component("Modal", move |mut args, window, cx| {
            let open = args.props().get("open").and_then(HostValue::as_bool) == Some(true);
            let mut current = state.borrow_mut();
            if current.open != open {
                current.open = open;
                if open {
                    current.previous = window.focused(cx).map(|focus| focus.downgrade());
                }
                let owner = Rc::downgrade(&state);
                window.defer(cx, move |window, cx| {
                    let Some(owner) = owner.upgrade() else { return; };
                    let mut current = owner.borrow_mut();
                    if current.open != open { return; }
                    if open {
                        current.focus.focus(window, cx);
                    } else if let Some(previous) = current.previous.take().and_then(|focus| focus.upgrade()) {
                        previous.focus(window, cx);
                    }
                });
            }
            if !open { return div().into_any_element(); }
            let focus = current.focus.clone();
            drop(current);
            let id = args.id().to_owned();
            let closed = sender.clone();
            let colors = cx.theme().semantic_tokens().colors;
            let form = args.props().get("form").and_then(HostValue::as_bool) == Some(true);
            let dismissable = args.props().get("dismissable").and_then(HostValue::as_bool).unwrap_or(true);
            let popup = if form {
                let title = args.props().get("title").and_then(HostValue::as_str).unwrap_or_default().to_owned();
                let width = args.props().get("width").and_then(HostValue::as_number)
                    .filter(|width| width.is_finite() && *width > 0. && *width <= f32::MAX as f64)
                    .unwrap_or(560.) as f32;
                form_popup(id.clone(), title, width, args.take_children(), dismissable, window, cx)
            } else {
                div().absolute().inset_0().flex().items_center().justify_center()
                    .child(component::surface::render(div().occlude().relative().flex().flex_col().p_5().pt_10()
                        .bg(colors.surface.alpha(1.)).text_color(colors.surface_foreground)
                        .border_1().border_color(colors.border).rounded(cx.theme().radius_lg)
                        .children(args.take_children()).child(close_button(id.clone(), dismissable)), cx)).into_any_element()
            };
            base::Dialog::new(cx)
                .focus_handle(focus)
                .close_on_backdrop_press(!form)
                .on_cancel(move |_, _, _| dismissable)
                .on_ok(|_, _, _| false)
                .on_close(move |_, _, _| { let _ = closed.try_send(id.clone()); })
                .backdrop(div().absolute().inset_0().bg(cx.theme().overlay))
                .popup(popup)
                .into_any_element()
        })
        .component("Details", |args, _, cx| {
            let subtitle = args.props().get("subtitle").and_then(HostValue::as_str).unwrap_or_default();
            let content = args.props().get("content").and_then(HostValue::as_str).unwrap_or_default();
            crate::ui::details::content(args.id().to_owned().into(), subtitle.to_owned().into(), content.to_owned().into(), cx).into_any_element()
        })
        .async_function("modal_closed", move |_| {
            let receiver = receiver.clone();
            Ok(async move {
                receiver.lock().await.recv().await.map(HostValue::from)
                    .ok_or_else(|| HostError::new("plugin modal closed"))
            })
        })
        .declarations(r#"
            export const Tooltip: { new(id: string, props: { text: string }): import("gpui-kit").Element };
            export const Modal: { new(id: string, props: { open: boolean; form?: boolean; title?: string; width?: number; dismissable?: boolean }): import("gpui-kit").Element };
            /** Read-only Markdown details. Use as the body of Modal with form: true. */
            export const Details: { new(id: string, props: { subtitle?: string; content: string }): import("gpui-kit").Element };
            export function modal_closed(): Promise<string>;
        "#)
}
