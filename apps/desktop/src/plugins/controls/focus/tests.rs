use super::*;
use crate::plugins::tests::init;
use core::prelude::v1::test;
use gpui_kit::component::{
    Root,
    diff::{Diff, DiffFile, DiffState},
};

struct Harness {
    diff: Entity<DiffState>,
    sibling: FocusHandle,
    pending: Rc<RefCell<Request>>,
    stop: CancellationToken,
    visible: bool,
}

impl Render for Harness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(div().track_focus(&self.sibling).child("Other control"))
            .child(render(
                "entry".into(),
                if self.visible {
                    vec![
                        Diff::new(&self.diff)
                            .header_visible(false)
                            .into_any_element(),
                    ]
                } else {
                    Vec::new()
                },
                self.pending.clone(),
                self.stop.clone(),
                window,
                cx,
            ))
    }
}

fn diff(cx: &mut App) -> Entity<DiffState> {
    let files =
        DiffFile::parse("--- a/notes.txt\n+++ b/notes.txt\n@@ -1 +1 @@\n-old\n+new\n").unwrap();
    cx.new(|cx| DiffState::new(files, cx))
}

fn mount(cx: &mut TestAppContext) -> (Entity<Harness>, &mut VisualTestContext) {
    init(cx);
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let diff = diff(cx);
        let view = cx.new(|cx| Harness {
            diff,
            sibling: cx.focus_handle().tab_stop(true),
            pending: Rc::new(RefCell::new(Request::default())),
            stop: CancellationToken::new(),
            visible: true,
        });
        owner = Some(view.clone());
        Root::new(view, window, cx)
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    (owner.unwrap(), visual)
}

#[gpui::test]
fn enters_replaced_native_leaf(cx: &mut TestAppContext) {
    let (owner, visual) = mount(cx);
    let target = visual.update(|window, cx| {
        owner.update(cx, |view, cx| {
            window.focus(&view.sibling, cx);
            view.diff = diff(cx);
            *view.pending.borrow_mut() = Request {
                target: Some("entry".into()),
                sequence: 1,
            };
            cx.notify();
            view.diff.read(cx).focus_handle(cx)
        })
    });
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        window.simulate_next_frame(cx);
        assert!(target.is_focused(window));
    });
}

#[gpui::test]
fn cancellation_preserves_other_focus(cx: &mut TestAppContext) {
    let (owner, visual) = mount(cx);
    visual.update(|window, cx| {
        owner.update(cx, |view, cx| {
            window.focus(&view.sibling, cx);
            *view.pending.borrow_mut() = Request {
                target: Some("entry".into()),
                sequence: 1,
            };
            cx.notify();
        });
        window.draw(cx).clear(cx);
        owner.update(cx, |view, _| {
            let mut request = view.pending.borrow_mut();
            request.target = None;
            request.sequence += 1;
        });
        window.simulate_next_frame(cx);
        assert!(owner.read(cx).sibling.is_focused(window));
    });
}

#[gpui::test]
fn missing_leaf_does_not_escape(cx: &mut TestAppContext) {
    let (owner, visual) = mount(cx);
    visual.update(|window, cx| {
        let leaf = owner.update(cx, |view, cx| {
            window.focus(&view.sibling, cx);
            view.visible = false;
            *view.pending.borrow_mut() = Request {
                target: Some("entry".into()),
                sequence: 1,
            };
            cx.notify();
            view.diff.read(cx).focus_handle(cx)
        });
        window.draw(cx).clear(cx);
        window.simulate_next_frame(cx);
        assert!(!owner.read(cx).sibling.is_focused(window));
        assert!(!leaf.is_focused(window));
        assert!(window.focused(cx).is_some());
    });
}
