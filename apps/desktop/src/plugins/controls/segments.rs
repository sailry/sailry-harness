//! Keep segmented-tab state and events inside the native Kit control.
use crate::plugins::host::sdk::values::decode;
use gpui_kit::prelude::FluentBuilder as _;
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
    #[serde(default)]
    fill: bool,
}

fn render(id: String, props: Props, events: Sender<Value>) -> AnyElement {
    let ids = props
        .items
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    TabBar::new(SharedString::from(id.clone()))
        .segmented()
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
                .when(props.fill, |tab| tab.flex_1().min_w_0())
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
        fill: bool,
        events: Sender<Value>,
    }
    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(400.)).p_4().child(render(
                "engine".into(),
                Props {
                    selected: Some("sqlite".into()),
                    disabled: self.disabled,
                    fill: self.fill,
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
    fn native_tabs_preserve_disabled_state(cx: &mut TestAppContext) {
        init(cx);
        let (events, mut receiver) = tokio::sync::mpsc::channel(4);
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|_| Harness {
                disabled: true,
                fill: true,
                events,
            });
            owner = Some(view.clone());
            Root::new(view, window, cx)
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
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

    #[gpui::test]
    fn fills_only_when_requested(cx: &mut TestAppContext) {
        use gpui_kit::test::TestWindowExt as _;

        init(cx);
        let props: Props = serde_json::from_value(json!({"items":[]})).unwrap();
        assert!(!props.fill);
        let (events, _receiver) = tokio::sync::mpsc::channel(4);
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|_| Harness {
                disabled: false,
                fill: false,
                events,
            });
            owner = Some(view.clone());
            Root::new(view, window, cx)
        });
        let bounds = |visual: &mut VisualTestContext| {
            visual.update(|window, cx| window.render_frame(cx));
            [
                "engine-item-sqlite",
                "engine-item-mysql",
                "engine-item-postgres",
            ]
            .map(|id| visual.debug_bounds(id).expect("native tab bounds"))
        };
        let natural = bounds(visual);
        assert!(natural[2].size.width > natural[0].size.width);
        visual.update(|_, cx| {
            owner.as_ref().unwrap().update(cx, |view, cx| {
                view.fill = true;
                cx.notify();
            });
        });
        let filled = bounds(visual);
        for (index, tab) in filled.iter().enumerate() {
            assert!((tab.size.width - filled[0].size.width).abs() <= px(1.));
            assert!(tab.size.width > natural[index].size.width);
        }
        visual.update(|_, cx| {
            owner.as_ref().unwrap().update(cx, |view, cx| {
                view.fill = false;
                cx.notify();
            });
        });
        assert_eq!(bounds(visual), natural);
        visual.update(|window, _| window.remove_window());
    }
}
