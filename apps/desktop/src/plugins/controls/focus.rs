//! Shell exposes no native Editor/Diff focus or after-frame callback. Anchor a
//! request at its leaf, then use public GPUI traversal after that layout paints.
use gpui_kit::*;
use gpui_shell::{HostError, HostModule, HostValue};
use sailry_link::CancellationToken;
use std::{cell::RefCell, rc::Rc};

#[cfg(test)]
mod tests;

#[derive(Default)]
struct Request {
    target: Option<String>,
    sequence: u64,
}

struct Entry {
    anchor: FocusHandle,
}

pub(super) fn extend(module: HostModule, stop: CancellationToken) -> HostModule {
    let pending = Rc::new(RefCell::new(Request::default()));
    let requests = pending.clone();
    module
        .function("focusEntry", move |args| {
            let target = match args.get(0) {
                Some(HostValue::Str(id)) if !id.is_empty() => Some(id.clone()),
                Some(HostValue::Null) => None,
                _ => return Err(HostError::new("focusEntry expects an entry ID or null")),
            };
            let mut request = requests.borrow_mut();
            request.target = target;
            request.sequence = request.sequence.wrapping_add(1);
            Ok(HostValue::Null)
        })
        .component("FocusEntry", move |mut args, window, cx| {
            render(
                args.id().to_owned(),
                args.take_children(),
                pending.clone(),
                stop.clone(),
                window,
                cx,
            )
        })
}

fn render(
    id: String,
    children: Vec<AnyElement>,
    pending: Rc<RefCell<Request>>,
    stop: CancellationToken,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let state = window.use_keyed_state(
        (
            ElementId::Name(SharedString::from(id.clone())),
            "focus-entry",
        ),
        cx,
        |_, cx| Entry {
            anchor: cx.focus_handle().tab_stop(false),
        },
    );
    let sequence = {
        let mut request = pending.borrow_mut();
        if request.target.as_deref() == Some(id.as_str()) {
            request.target = None;
            Some(request.sequence)
        } else {
            None
        }
    };
    if let Some(sequence) = sequence {
        let state = state.downgrade();
        // The current materialization must finish painting before traversal can
        // see its native tab stop rather than the surface being replaced.
        window.on_next_frame(move |window, cx| {
            if stop.is_cancelled() || pending.borrow().sequence != sequence {
                return;
            }
            let Some(state) = state.upgrade() else {
                return;
            };
            let anchor = state.read(cx).anchor.clone();
            window.focus(&anchor, cx);
            window.focus_next(cx);
            // A missing or disabled native tab stop must not send keyboard
            // focus to another control elsewhere in the window.
            if !anchor.contains_focused(window, cx) {
                window.focus(&anchor, cx);
            }
        });
    }
    div()
        .id(SharedString::from(id))
        .size_full()
        .min_w_0()
        .min_h_0()
        .tab_group()
        .track_focus(&state.read(cx).anchor)
        .children(children)
        .into_any_element()
}
