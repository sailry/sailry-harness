//! Scripts cannot read element bounds. A zero-height, full-width canvas
//! reports its parent's width as `{id, value}` whenever the rounded width
//! changes, so plugin toolbars can collapse controls into an overflow menu.
use gpui_kit::*;
use gpui_shell::HostModule;
use serde_json::{Value, json};
use std::{cell::Cell, rc::Rc};
use tokio::sync::mpsc::Sender;

/// Records the width and reports whether it differs from the last one.
fn changed(last: &Cell<Option<u32>>, bounds: Bounds<Pixels>) -> Option<u32> {
    let width: f32 = bounds.size.width.into();
    let width = width.max(0.).round() as u32;
    (last.replace(Some(width)) != Some(width)).then_some(width)
}

pub(super) fn extend(module: HostModule, events: Sender<Value>) -> HostModule {
    module.component("Measure", move |args, window, cx| {
        let id = args.id().to_owned();
        let last = window
            .use_keyed_state(
                (ElementId::from(SharedString::from(id.clone())), "measure"),
                cx,
                |_, _| Rc::new(Cell::new(None)),
            )
            .read(cx)
            .clone();
        let events = events.clone();
        div()
            .w_full()
            .h(px(0.))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, _, _| {
                        if let Some(width) = changed(&last, bounds) {
                            let _ = events.try_send(json!({"id":id,"value":width}));
                        }
                    },
                )
                .size_full(),
            )
            .into_any_element()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn reports_only_changes() {
        let last = Cell::new(None);
        let bounds = |width: f32| Bounds::new(point(px(0.), px(0.)), size(px(width), px(0.)));
        assert_eq!(changed(&last, bounds(320.2)), Some(320));
        assert_eq!(changed(&last, bounds(319.8)), None);
        assert_eq!(changed(&last, bounds(500.)), Some(500));
    }
}
