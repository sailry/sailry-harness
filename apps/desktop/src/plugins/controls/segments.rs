//! Kit b79f4ce omits TabBar.equal_width from scripts. Keep native segmented tabs.
use crate::plugins::host::sdk::values::decode;
use gpui_kit::{
    component::tab::{Tab, TabBar},
    *,
};
use gpui_shell::{HostError, HostModule};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc::Sender;

#[derive(Deserialize)]
struct Item {
    id: String,
    label: String,
}

#[derive(Deserialize)]
struct Props {
    items: Vec<Item>,
    selected: Option<String>,
    #[serde(default)]
    disabled: bool,
}

fn render(id: String, props: Props, events: Sender<Value>) -> AnyElement {
    let ids = props
        .items
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    TabBar::new(SharedString::from(id.clone()))
        .segmented()
        .equal_width()
        .w_full()
        .menu(false)
        .selected_index(
            ids.iter()
                .position(|value| Some(value) == props.selected.as_ref())
                .unwrap_or(usize::MAX),
        )
        .children(props.items.into_iter().map(|item| {
            let selector = format!("{id}-item-{}", item.id);
            Tab::new()
                .label(item.label)
                .disabled(props.disabled)
                .debug_selector(move || selector.clone())
        }))
        .on_click(move |index, _, _| {
            if !props.disabled
                && let Some(value) = ids.get(*index)
            {
                let _ = events.try_send(json!({"id":id,"value":value}));
            }
        })
        .into_any_element()
}

pub(super) fn extend(module: HostModule, events: Sender<Value>) -> HostModule {
    module.component("SegmentedTabs", move |args, _, _| {
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

    struct Harness {
        disabled: bool,
        events: Sender<Value>,
    }
    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(400.)).p_4().child(render(
                "engine".into(),
                Props {
                    selected: Some("sqlite".into()),
                    disabled: self.disabled,
                    items: vec![
                        Item {
                            id: "sqlite".into(),
                            label: "SQLite".into(),
                        },
                        Item {
                            id: "mysql".into(),
                            label: "MySQL".into(),
                        },
                        Item {
                            id: "postgres".into(),
                            label: "PostgreSQL".into(),
                        },
                    ],
                },
                self.events.clone(),
            ))
        }
    }

    #[gpui::test]
    fn native_tabs_share_width_and_respect_disabled_state(cx: &mut TestAppContext) {
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
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let widths = [
            "engine-item-sqlite",
            "engine-item-mysql",
            "engine-item-postgres",
        ]
        .map(|selector| visual.debug_bounds(selector).unwrap().size.width);
        assert!((widths[0] - widths[1]).abs() < px(1.));
        assert!((widths[1] - widths[2]).abs() < px(1.));
        click(visual, "engine-item-postgres");
        assert!(receiver.try_recv().is_err());
        visual.update(|window, cx| {
            owner.as_ref().unwrap().update(cx, |view, cx| {
                view.disabled = false;
                cx.notify();
            });
            window.draw(cx).clear(cx);
        });
        click(visual, "engine-item-postgres");
        assert_eq!(
            receiver.try_recv().unwrap(),
            json!({"id":"engine","value":"postgres"})
        );
        assert!(receiver.try_recv().is_err());
        visual.update(|window, _| window.remove_window());
    }
}
