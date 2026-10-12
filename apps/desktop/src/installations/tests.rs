use super::*;
use core::prelude::v1::test;
use gpui_kit::component::{Root, WindowExt as _};
use sailry_protocol::installation::Task as Observed;

fn key() -> Key {
    (NodeId([1; 32]), RequestId::new())
}

fn entry() -> Entry {
    Entry {
        order: 1,
        resource: Resource::Plugin {
            name: "example".into(),
        },
        status: Presentation::Starting,
        retry: Some(Rc::new(|_, _, _| {})),
        finished: false,
        connected: true,
        dismissing: false,
    }
}

fn failure() -> Fault {
    Fault::new(ErrorCode::InvalidRequest, "invalid package")
}

fn observed(id: RequestId, state: Status) -> View {
    View {
        connected: true,
        error: None,
        tasks: vec![Observed {
            request: id,
            revision: 1,
            resource: Resource::Plugin {
                name: "example".into(),
            },
            state,
        }],
    }
}

#[gpui::test]
fn disappearance_does_not_complete_owned_requests(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let queue = registry(cx);
        let key = key();
        queue.update(cx, |queue, _| {
            queue.entries.insert(key, entry());
            queue.accept(
                key.0,
                View {
                    connected: true,
                    ..Default::default()
                },
            );
            assert!(!queue.entries[&key].finished);
        });
        Ticket {
            queue: queue.clone(),
            key,
        }
        .record(Ok(()), cx);
        assert!(queue.read(cx).entries.is_empty());
        assert!(!visible(cx));
    });
}

#[gpui::test]
fn actions_wait_for_original_completion(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let queue = registry(cx);
        let key = key();
        queue.update(cx, |queue, _| {
            queue.entries.insert(key, entry());
            queue.accept(key.0, observed(key.1, Status::Failed { error: failure() }));
            assert!(!queue.entries[&key].actionable());
        });
        Ticket {
            queue: queue.clone(),
            key,
        }
        .record(Err(failure()), cx);
        assert!(queue.read(cx).entries[&key].actionable());
        assert!(visible(cx));
    });
}

#[gpui::test]
fn recovers_unowned_terminal_tasks(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let queue = registry(cx);
        let key = key();
        queue.update(cx, |queue, _| {
            queue.accept(key.0, observed(key.1, Status::Failed { error: failure() }));
            let entry = &queue.entries[&key];
            assert!(entry.actionable());
            assert!(entry.retry.is_none());
        });
    });
}

#[gpui::test]
fn retains_uncertainty_after_unowned_disappearance(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let queue = registry(cx);
        let key = key();
        queue.update(cx, |queue, _| {
            queue.accept(key.0, observed(key.1, Status::Queued));
            queue.accept(
                key.0,
                View {
                    connected: true,
                    ..Default::default()
                },
            );
            assert!(matches!(
                queue.entries[&key].status,
                Presentation::Observed(Status::Uncertain { .. })
            ));
            assert!(queue.entries[&key].actionable());
        });
    });
}

struct Frame;
impl Render for Frame {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

#[gpui::test]
fn hide_reopens_the_same_list(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let frame = cx.new(|_| Frame);
        Root::new(frame, window, cx)
    });
    visual.update(|window, cx| {
        let queue = registry(cx);
        queue.update(cx, |queue, cx| {
            queue.entries.insert(key(), entry());
            queue.changed(cx);
            queue.open(window, cx);
            let list = queue.list.as_ref().unwrap().entity_id();
            queue.hide(window, cx);
            assert!(!window.has_active_dialog(cx));
            assert_eq!(queue.entries.len(), 1);
            queue.open(window, cx);
            assert_eq!(queue.list.as_ref().unwrap().entity_id(), list);
        });
    });
}

#[gpui::test]
fn completion_preserves_a_stacked_dialog(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let frame = cx.new(|_| Frame);
        Root::new(frame, window, cx)
    });
    let queue = visual.update(|window, cx| {
        let queue = registry(cx);
        queue.update(cx, |queue, cx| {
            queue.entries.insert(key(), entry());
            queue.changed(cx);
            queue.open(window, cx);
        });
        window.open_dialog(cx, |dialog, _, _| dialog.title("Confirmation"));
        let confirmation = window.focused(cx).unwrap();
        queue.update(cx, |queue, cx| {
            queue.entries.clear();
            queue.changed(cx);
            queue.close_empty(window, cx);
            assert!(confirmation.contains_focused(window, cx));
            assert!(window.has_active_dialog(cx));
        });
        window.close_dialog(cx);
        queue
    });
    visual.run_until_parked();
    visual.update(|window, cx| {
        assert!(queue.read(cx).window.is_none());
        assert!(!window.has_active_dialog(cx));
    });
}

struct Form {
    dialog: crate::ui::overlays::Owned,
}

#[gpui::test]
fn form_completion_preserves_the_task_dialog(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let frame = cx.new(|_| Frame);
        Root::new(frame, window, cx)
    });
    let form = visual.update(|window, cx| {
        let form = cx.new(|_| Form {
            dialog: Default::default(),
        });
        window.open_dialog(cx, |dialog, _, _| dialog.title("Install"));
        form.update(cx, |form, cx| {
            crate::ui::overlays::Owned::bind(form, |form| &mut form.dialog, window, cx)
        });
        let queue = registry(cx);
        queue.update(cx, |queue, cx| {
            queue.entries.insert(key(), entry());
            queue.changed(cx);
            queue.open(window, cx);
        });
        let progress = window.focused(cx).unwrap();
        form.update(cx, |form, cx| form.dialog.close(window, cx));
        assert!(progress.contains_focused(window, cx));
        assert!(window.has_active_dialog(cx));
        queue.update(cx, |queue, cx| queue.hide(window, cx));
        form
    });
    visual.run_until_parked();
    visual.update(|window, cx| {
        assert!(!form.read(cx).dialog.presented(cx));
        assert!(!window.has_active_dialog(cx));
        assert!(visible(cx));
    });
}
