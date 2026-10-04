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
                .child(component::surface::render(
                    div()
                        .id(SharedString::from(id.clone()))
                        .debug_selector(move || id.clone())
                        .occlude()
                        .min_h_0()
                        .p_0()
                        .overflow_hidden()
                        .w(width)
                        .max_h(window.viewport_size().height * 0.6)
                        .bg(cx.theme().tokens.background)
                        .text_color(cx.theme().foreground)
                        .border_1()
                        .border_color(cx.theme().border)
                        .rounded(cx.theme().surface_radius())
                        .shadow_xl()
                        .child(command),
                    cx,
                )),
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

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::component::input::{Input, InputState};

    struct Harness {
        id: String,
        open: bool,
        input: Entity<InputState>,
        events: Entity<Events>,
        pickers: Pickers,
    }

    impl Render for Harness {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(Input::new(&self.input))
                .child(super::super::render(
                    self.id.clone(),
                    Props {
                        open: self.open,
                        title: "Search".into(),
                        items: ["first", "second"]
                            .into_iter()
                            .map(|id| Item {
                                id: id.into(),
                                label: id.into(),
                                detail: format!("Detail {id}"),
                                icon: None,
                                group: String::new(),
                                checked: false,
                                disabled: false,
                                submenu: true,
                                value: Some(json!({"profile":id})),
                            })
                            .collect(),
                        loading: false,
                        empty: "No matches".into(),
                        mode: Mode::Command,
                    },
                    self.events.clone(),
                    &self.pickers,
                    window,
                    cx,
                ))
        }
    }

    fn draw(visual: &mut VisualTestContext) {
        for _ in 0..2 {
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
        }
        visual.run_until_parked();
    }

    #[gpui::test]
    fn filters_confirms_and_releases_with_native_focus(cx: &mut TestAppContext) {
        crate::plugins::tests::init(cx);
        let (sender, mut receiver) = tokio::sync::mpsc::channel(32);
        let pickers: Pickers = Rc::new(RefCell::new(BTreeMap::new()));
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let input = cx.new(|cx| InputState::new(window, cx).default_value("Draft"));
            let events = cx.new(|_| Events::new(sender, CancellationToken::new()));
            let view = cx.new(|_| Harness {
                id: "command-1".into(),
                open: false,
                input: input.clone(),
                events,
                pickers: pickers.clone(),
            });
            owner = Some((view.clone(), input));
            Root::new(view, window, cx)
        });
        let (view, input) = owner.unwrap();
        visual.update(|window, cx| input.update(cx, |input, cx| input.focus(window, cx)));
        draw(visual);
        view.update(visual, |view, cx| {
            view.open = true;
            cx.notify();
        });
        draw(visual);
        let current = visual.update(|window, cx| {
            let owner = pickers.borrow()["command-1"].upgrade().unwrap();
            let Control::Command(command) = &owner.read(cx).control else {
                panic!("command picker expected");
            };
            assert!(command.focus_handle(cx).is_focused(window));
            command.downgrade()
        });
        let viewport = visual.update(|window, _| window.viewport_size());
        let bounds = visual.debug_bounds("command-1").unwrap();
        let width = px(480.).min(viewport.width - px(40.));
        assert_eq!(bounds.size.width, width);
        assert_eq!(bounds.left(), (viewport.width - width) / 2.);
        assert_eq!(bounds.top(), viewport.height / 10.);
        assert!(bounds.size.height <= viewport.height * 0.6);
        assert!(visual.debug_bounds("second").is_some());
        visual.simulate_input("Detail second");
        visual
            .executor()
            .advance_clock(std::time::Duration::from_millis(100));
        draw(visual);
        visual.simulate_keystrokes("enter");
        visual.run_until_parked();
        assert_eq!(
            receiver.try_recv().unwrap(),
            json!({"picker":"command-1","kind":"select","id":"second","value":{"profile":"second"}})
        );
        view.update(visual, |view, cx| {
            view.id = "command-2".into();
            cx.notify();
        });
        draw(visual);
        assert!(current.upgrade().is_none());
        visual.update(|window, cx| {
            let owner = pickers.borrow()["command-2"].upgrade().unwrap();
            let Control::Command(command) = &owner.read(cx).control else {
                panic!("command picker expected");
            };
            assert!(command.focus_handle(cx).is_focused(window));
            assert!(command.read(cx).query(cx).is_empty());
        });
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        assert_eq!(
            receiver.try_recv().unwrap(),
            json!({"picker":"command-2","kind":"close"})
        );
        view.update(visual, |view, cx| {
            view.open = false;
            cx.notify();
        });
        draw(visual);
        assert!(pickers.borrow().is_empty());
        assert!(visual.debug_bounds("command-2").is_none());
        assert!(visual.update(|window, cx| input.focus_handle(cx).is_focused(window)));
        assert_eq!(input.read_with(visual, |input, _| input.value()), "Draft");
    }
}
