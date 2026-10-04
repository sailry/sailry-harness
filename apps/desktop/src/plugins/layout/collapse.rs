//! Kit's script Collapsible inserts its content before its header. Keep named
//! slots and expansion in this native adapter without changing the framework.
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants as _},
        collapsible::Collapsible,
        *,
    },
    prelude::FluentBuilder as _,
    *,
};
use gpui_shell::{HostModule, HostValue};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Header,
    Content,
    Actions,
}

struct Slot {
    kind: Option<Kind>,
    content: AnyElement,
}

struct Expanded {
    open: Rc<Cell<bool>>,
    actions: FocusHandle,
    _focus: [Subscription; 2],
}

pub(super) fn extend(module: HostModule) -> HostModule {
    // Retain choices across tabs and hidden project groups for this controller.
    let retained = Rc::new(RefCell::new(BTreeMap::<String, Rc<Cell<bool>>>::new()));
    let choices = retained.clone();
    let declarations = format!(
        "{}\n{}",
        module.declared().unwrap_or_default(),
        r#"
            /** Host-owned expansion. Exactly one header and content slot, and at most one actions slot. */
            export const Collapse: { new(id: string, props: { label: string; default_open?: boolean; variant?: "panel" | "plain" }): import("gpui-kit").Element };
            /** Arbitrary Kit children; slot placement is independent of construction order. */
            export const CollapseSlot: { new(id: string, props: { variant: "header" | "content" | "actions" }): import("gpui-kit").Element };
            /** Current host choice, or null before this group is materialized. */
            export function collapseOpen(id: string): boolean | null;
        "#
    );
    module
        .function("collapseOpen", move |args| {
            Ok(choices
                .borrow()
                .get(args.string(0)?)
                .map(|open| HostValue::Bool(open.get()))
                .unwrap_or(HostValue::Null))
        })
        .component("CollapseSlot", |mut args, _, _| {
            let kind = match args.props().get("variant").and_then(HostValue::as_str) {
                Some("header") => Some(Kind::Header),
                Some("content") => Some(Kind::Content),
                Some("actions") => Some(Kind::Actions),
                _ => None,
            };
            let id = args.id().to_owned();
            Slot {
                kind,
                content: div()
                    .id(id.clone())
                    .debug_selector(move || id.clone())
                    .w_full()
                    .min_w_0()
                    .children(args.take_children())
                    .into_any_element(),
            }
            .into_any_element()
        })
        .component("Collapse", move |mut args, window, cx| {
            let id = args.id().to_owned();
            let label = args
                .props()
                .get("label")
                .and_then(HostValue::as_str)
                .unwrap_or_default()
                .to_owned();
            let mut header = None;
            let mut content = None;
            let mut actions = None;
            let mut valid = true;
            for mut child in args.take_children() {
                let Some(slot) = child.downcast_mut::<Slot>() else {
                    valid = false;
                    continue;
                };
                let target = match slot.kind {
                    Some(Kind::Header) => &mut header,
                    Some(Kind::Content) => &mut content,
                    Some(Kind::Actions) => &mut actions,
                    None => {
                        valid = false;
                        continue;
                    }
                };
                if target.is_some() {
                    valid = false;
                } else {
                    *target = Some(std::mem::replace(
                        &mut slot.content,
                        div().into_any_element(),
                    ));
                }
            }
            let (Some(header), Some(content)) = (header, content) else {
                invalid(&id, window, cx);
                return div().into_any_element();
            };
            if !valid {
                invalid(&id, window, cx);
                return div().into_any_element();
            }
            let initial = args
                .props()
                .get("default_open")
                .and_then(HostValue::as_bool)
                == Some(true);
            let choice = retained
                .borrow_mut()
                .entry(id.clone())
                .or_insert_with(|| Rc::new(Cell::new(initial)))
                .clone();
            let state = window.use_keyed_state(
                (ElementId::Name(SharedString::from(id.clone())), "expanded"),
                cx,
                |window, cx| {
                    let actions = cx.focus_handle().tab_stop(false);
                    Expanded {
                        open: choice,
                        _focus: [
                            cx.on_focus_in(&actions, window, |_, _, cx| cx.notify()),
                            cx.on_focus_out(&actions, window, |_, _, _, cx| cx.notify()),
                        ],
                        actions,
                    }
                },
            );
            let open = state.read(cx).open.get();
            let actions_focus = state.read(cx).actions.clone();
            let actions_visible = actions_focus.contains_focused(window, cx);
            let actions_selector = format!(
                "{id}-actions-{}",
                if actions_visible { "focused" } else { "hover" }
            );
            let plain = args.props().get("variant").and_then(HostValue::as_str) == Some("plain");
            let group: SharedString = format!("collapse-{id}").into();
            let button = Button::new(id.clone())
                .debug_selector({
                    let id = id.clone();
                    move || id.clone()
                })
                .ghost()
                .small()
                .w_full()
                .flex_1()
                .h_9()
                .min_w_0()
                .when(plain, |button| button.h_8().px_1())
                .when(!plain, |button| button.rounded_none().bg(cx.theme().muted))
                .justify_start()
                .accessibility_label(label)
                .icon(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .child(div().flex_1().min_w_0().text_sm().child(header))
                .on_click(move |_, _, cx| {
                    state.update(cx, |state, cx| {
                        state.open.set(!state.open.get());
                        cx.notify();
                    });
                });
            Collapsible::new()
                .open(open)
                .w_full()
                .min_w_0()
                .flex_shrink_0()
                .when(!plain, |panel| {
                    panel
                        .border_1()
                        .border_color(cx.theme().border)
                        .rounded(cx.theme().radius)
                        .overflow_hidden()
                })
                .child(
                    h_flex()
                        .group(group.clone())
                        .relative()
                        .w_full()
                        .min_w_0()
                        .child(button)
                        .when_some(actions, |row, actions| {
                            row.child(
                                div()
                                    .debug_selector(move || actions_selector)
                                    .flex_shrink_0()
                                    .mr_2()
                                    .track_focus(&actions_focus)
                                    .opacity(if actions_visible { 1. } else { 0. })
                                    .group_hover(group, |style| style.opacity(1.))
                                    .child(actions),
                            )
                        }),
                )
                .content(div().w_full().min_w_0().text_sm().child(content))
                .into_any_element()
        })
        .declarations(declarations)
}

fn invalid(id: &str, window: &mut Window, cx: &mut App) {
    let presented = window.use_keyed_state(
        (
            ElementId::Name(SharedString::from(id.to_owned())),
            "slot-error",
        ),
        cx,
        |_, _| false,
    );
    if *presented.read(cx) {
        return;
    }
    presented.update(cx, |presented, _| *presented = true);
    let text = crate::tr("plugins_slots_invalid");
    crate::feedback::toast(
        window,
        text.clone(),
        notification::Notification::error(text).id1::<Slot>(SharedString::from(id.to_owned())),
        cx,
    );
}

impl IntoElement for Slot {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Slot {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.content.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.content.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.content.paint(window, cx);
    }
}
