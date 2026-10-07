use super::*;
use core::prelude::v1::test;
use sailry_protocol::{
    Delegation, WorktreeId,
    conversation::{Run, RunKind},
};

struct Harness {
    children: Vec<Child>,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex().child(icons(&self.children.iter().collect::<Vec<_>>(), cx))
    }
}

fn children(count: usize) -> Vec<Child> {
    let session = SessionId::new();
    let turn = TurnId::new();
    (0..count)
        .map(|index| Child {
            run: Run {
                worktree: WorktreeId::new(),
                turn: TurnId::new(),
                kind: RunKind::Task,
                session: SessionId::new(),
                sequence: index as u64,
                revision: 1,
                status: Status::Completed,
                error: None,
                started_ms: None,
                finished_ms: None,
                origin: None,
            },
            origin: Delegation {
                session,
                turn,
                entry: "delegation".into(),
                index,
                role: None,
            },
            name: None,
        })
        .collect()
}

fn mount<'a>(
    cx: &'a mut TestAppContext,
    count: usize,
) -> (Entity<Harness>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Harness {
            children: children(count),
        });
        entity = Some(view.clone());
        Root::new(view, window, cx)
    });
    (entity.unwrap(), visual)
}

#[gpui::test]
fn preserves_count_and_order(cx: &mut TestAppContext) {
    let (view, visual) = mount(cx, 2);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for count in [2, 3, 8] {
            visual.update(|window, cx| Theme::change(mode, Some(window), cx));
            view.update(visual, |view, cx| {
                view.children = children(count);
                cx.notify();
            });
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let keys = view.read_with(visual, |view, _| {
                view.children.iter().map(key).collect::<Vec<_>>()
            });
            let mut previous = None;
            for key in keys {
                let bounds = visual
                    .debug_bounds(Box::leak(
                        format!("live-subagent-icon-{key}").into_boxed_str(),
                    ))
                    .unwrap();
                assert_eq!(bounds.size, size(px(16.), px(16.)));
                if let Some(previous) = previous {
                    assert_eq!(bounds.left(), previous + px(4.));
                }
                previous = Some(bounds.right());
            }
        }
    }
    visual.update(|window, _| window.remove_window());
}

#[gpui::test]
fn rotates_each_active_child(cx: &mut TestAppContext) {
    let (view, visual) = mount(cx, 2);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for reduced in [false, true] {
            visual.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                cx.set_reduce_motion(reduced);
            });
            for states in [
                [Status::Completed, Status::Completed],
                [Status::Running, Status::Completed],
                [Status::Completed, Status::Running],
                [Status::Stopping, Status::Running],
                [Status::Queued, Status::Completed],
                [Status::Failed, Status::Interrupted],
                [Status::Cancelled, Status::Completed],
            ] {
                view.update(visual, |view, cx| {
                    for (child, state) in view.children.iter_mut().zip(states) {
                        child.run.status = state;
                    }
                    cx.notify();
                });
                // No shimmer or other animation is mounted in this harness.
                let frames = visual.update(|window, cx| {
                    window.simulate_next_frame(cx);
                    window.refresh();
                    window.draw(cx).clear(cx);
                    window.simulate_next_frame(cx)
                });
                assert_eq!(
                    frames > 0,
                    states
                        .iter()
                        .any(|state| matches!(state, Status::Running | Status::Stopping))
                        && !reduced
                );
                let keys = view.read_with(visual, |view, _| {
                    view.children.iter().map(key).collect::<Vec<_>>()
                });
                for (key, state) in keys.into_iter().zip(states) {
                    let running = matches!(state, Status::Running | Status::Stopping);
                    let selector = format!(
                        "live-subagent-icon-{key}-{}",
                        if running { "rotating" } else { "static" }
                    );
                    let bounds = visual
                        .debug_bounds(Box::leak(selector.into_boxed_str()))
                        .unwrap();
                    assert_eq!(bounds.size, size(px(16.), px(16.)));
                }
            }
        }
    }
    visual.update(|window, _| window.remove_window());
}
