//! List-end sentinel for incremental plugin pages. Kit lists expose no
//! end-reached callback for script scroll containers, so a 1px canvas compares
//! its painted bounds with the current content mask and reports `{id, value:
//! "load"}` once each time it enters the visible region.
use crate::plugins::host::sdk::values::decode;
use gpui_kit::{
    component::{ActiveTheme as _, Sizable as _, h_flex, spinner::Spinner},
    *,
};
use gpui_shell::{HostError, HostModule};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{cell::Cell, rc::Rc};
use tokio::sync::mpsc::Sender;

#[derive(Deserialize)]
struct Props {
    loading: bool,
    label: String,
}

/// Records the sentinel's current visibility and reports whether it just entered the mask.
fn entered(seen: &Cell<bool>, bounds: Bounds<Pixels>, mask: Bounds<Pixels>) -> bool {
    let clipped = bounds.intersect(&mask);
    let visible = clipped.size.width > px(0.) && clipped.size.height > px(0.);
    !seen.replace(visible) && visible
}

fn render(
    id: String,
    props: Props,
    events: Sender<Value>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let seen = window
        .use_keyed_state(
            (ElementId::from(SharedString::from(id.clone())), "load-more"),
            cx,
            |_, _| Rc::new(Cell::new(false)),
        )
        .read(cx)
        .clone();
    let selector = id.clone();
    if props.loading {
        // A completed page that leaves the sentinel visible must request the next one.
        seen.set(false);
        return h_flex()
            .debug_selector(move || selector.clone())
            .w_full()
            .justify_center()
            .gap_2()
            .py_2()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(Spinner::new().small())
            .child(props.label)
            .into_any_element();
    }
    div()
        .debug_selector(move || selector.clone())
        .w_full()
        .h(px(1.))
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    if entered(&seen, bounds, window.content_mask().bounds) {
                        let _ = events.try_send(json!({"id":id,"value":"load"}));
                    }
                },
            )
            .size_full(),
        )
        .into_any_element()
}

pub(super) fn extend(module: HostModule, events: Sender<Value>) -> HostModule {
    module.component("LoadMore", move |args, window, cx| {
        let Ok(props) = decode(args.props()).and_then(|value| {
            serde_json::from_value::<Props>(value)
                .map_err(|error| HostError::new(error.to_string()))
        }) else {
            return div().into_any_element();
        };
        render(args.id().to_owned(), props, events.clone(), window, cx)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn row(y: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(0.), px(y)), size(px(100.), px(1.)))
    }

    #[test]
    fn reports_entry_once() {
        let mask = Bounds::new(point(px(0.), px(0.)), size(px(100.), px(50.)));
        let seen = Cell::new(false);
        assert!(!entered(&seen, row(80.), mask));
        assert!(entered(&seen, row(49.), mask));
        assert!(!entered(&seen, row(20.), mask));
        assert!(!entered(&seen, row(50.), mask));
        assert!(entered(&seen, row(0.), mask));
    }

    #[test]
    fn rearms_after_reset() {
        let mask = Bounds::new(point(px(0.), px(0.)), size(px(100.), px(50.)));
        let seen = Cell::new(false);
        assert!(entered(&seen, row(10.), mask));
        seen.set(false);
        assert!(entered(&seen, row(10.), mask));
    }
}
