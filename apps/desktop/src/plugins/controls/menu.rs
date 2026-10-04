//! Kit b79f4ce's script DropdownMenu omits caret, checked items and disabled state.
//! Keep the original native button-triggered popup rather than substituting Select.
use crate::plugins::host::sdk::values::decode;
use gpui_kit::{
    component::{
        Disableable as _,
        button::Button,
        menu::{DropdownMenu, PopupMenuItem},
    },
    *,
};
use gpui_shell::{HostError, HostModule};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc::Sender;

#[derive(Clone, Deserialize)]
struct Item {
    id: String,
    label: String,
    #[serde(default)]
    disabled: bool,
}

#[derive(Deserialize)]
struct Props {
    label: String,
    items: Vec<Item>,
    selected: Option<String>,
    #[serde(default)]
    disabled: bool,
}

fn item(id: &str, value: &Item, selected: Option<&str>, events: Sender<Value>) -> PopupMenuItem {
    let control = id.to_owned();
    let value_id = value.id.clone();
    PopupMenuItem::new(value.label.clone())
        .checked(selected == Some(value.id.as_str()))
        .disabled(value.disabled)
        .on_click(move |_, _, _| {
            let _ = events.try_send(json!({"id":control,"value":value_id}));
        })
}

fn render(id: String, props: Props, events: Sender<Value>) -> AnyElement {
    let button = Button::new(SharedString::from(id.clone()))
        .label(props.label)
        .dropdown_caret(true)
        .disabled(props.disabled)
        .debug_selector({
            let id = id.clone();
            move || id.clone()
        });
    if props.disabled {
        return button.into_any_element();
    }
    button
        .dropdown_menu(move |menu, _, _| {
            props.items.iter().fold(menu, |menu, value| {
                menu.item(item(&id, value, props.selected.as_deref(), events.clone()))
            })
        })
        .into_any_element()
}

pub(super) fn extend(module: HostModule, events: Sender<Value>) -> HostModule {
    module.component("MenuButton", move |args, _, _| {
        let Ok(props) = decode(args.props()).and_then(|value| {
            serde_json::from_value::<Props>(value)
                .map_err(|error| HostError::new(error.to_string()))
        }) else {
            return div().into_any_element();
        };
        render(args.id().to_owned(), props, events.clone())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::tests::{click, init};
    use core::prelude::v1::test;
    use gpui_kit::component::Root;

    fn items() -> Vec<Item> {
        vec![
            Item {
                id: "require".into(),
                label: "Require".into(),
                disabled: false,
            },
            Item {
                id: "prefer".into(),
                label: "Prefer".into(),
                disabled: true,
            },
            Item {
                id: "disable".into(),
                label: "Disabled".into(),
                disabled: false,
            },
        ]
    }

    #[test]
    fn preserves_checked_and_disabled_items() {
        let (events, _) = tokio::sync::mpsc::channel(4);
        for (index, value) in items().iter().enumerate() {
            let PopupMenuItem::Item {
                checked, disabled, ..
            } = item("tls", value, Some("prefer"), events.clone())
            else {
                panic!("native menu item expected");
            };
            assert_eq!(checked, index == 1);
            assert_eq!(disabled, index == 1);
        }
    }

    struct Harness {
        disabled: bool,
        events: Sender<Value>,
    }
    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(400.)).p_4().child(render(
                "tls".into(),
                Props {
                    label: "Require".into(),
                    items: items(),
                    selected: Some("require".into()),
                    disabled: self.disabled,
                },
                self.events.clone(),
            ))
        }
    }

    #[gpui::test]
    fn native_trigger_and_items_respect_disabled_state(cx: &mut TestAppContext) {
        init(cx);
        let (events, mut receiver) = tokio::sync::mpsc::channel(4);
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|_| Harness {
                disabled: true,
                events,
            });
            owner = Some(view.clone());
            Root::new(view, window, cx)
        });
        click(visual, "tls");
        visual.simulate_keystrokes("down");
        visual.simulate_keystrokes("enter");
        visual.run_until_parked();
        assert!(receiver.try_recv().is_err());
        visual.update(|window, cx| {
            owner.as_ref().unwrap().update(cx, |view, cx| {
                view.disabled = false;
                cx.notify();
            });
            window.draw(cx).clear(cx);
        });
        click(visual, "tls");
        visual.update(|window, cx| window.draw(cx).clear(cx));
        visual.simulate_keystrokes("down");
        visual.simulate_keystrokes("down");
        visual.simulate_keystrokes("enter");
        visual.run_until_parked();
        assert_eq!(
            receiver.try_recv().unwrap(),
            json!({"id":"tls","value":"disable"})
        );
        assert!(receiver.try_recv().is_err());
        visual.update(|window, _| window.remove_window());
    }
}
