use super::*;
use crate::conversation::live::tests::fixture as chat;
use core::prelude::v1::test;
use gpui_kit::test::TestWindowExt as _;
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

struct Group {
    owner: Entity<View>,
    turn: TurnId,
    children: Vec<Child>,
    continuing: bool,
}

impl Render for Group {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let children = self.children.iter().collect::<Vec<_>>();
        self.owner.update(cx, |view, cx| {
            view.render_children(self.turn, &children, self.continuing, cx)
        })
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

fn mount(cx: &mut TestAppContext, count: usize) -> (Entity<Harness>, &mut VisualTestContext) {
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

fn mount_group(
    cx: &mut TestAppContext,
    binding: Binding,
    children: Vec<Child>,
) -> (Entity<Group>, &mut VisualTestContext) {
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let owner = cx.new(|cx| View::new(binding, None, window, cx));
        let view = cx.new(|cx| {
            cx.observe(&owner, |_, _, cx| cx.notify()).detach();
            Group {
                owner,
                turn: TurnId::new(),
                children,
                continuing: false,
            }
        });
        entity = Some(view.clone());
        Root::new(view, window, cx)
    });
    (entity.unwrap(), visual)
}

#[gpui::test]
fn shows_each_session_once(cx: &mut TestAppContext) {
    chat::init(cx);
    let fixture = crate::conversation::live::ChatFixture::with_tools(false, Vec::new());
    let records = children(3);
    let (view, visual) = mount_group(cx, fixture.binding.clone(), records.clone());
    let owner = view.read_with(visual, |view, _| view.owner.clone());
    let first = key(&records[0]);
    let toggle = format!("live-turn-subagents-{first}");
    let list = Box::leak(format!("live-subagent-list-{first}").into_boxed_str());
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        visual.update(|window, cx| Theme::change(mode, Some(window), cx));
        for reuse in [true, false] {
            for (earlier, status) in [
                (Status::Completed, Status::Completed),
                (Status::Queued, Status::Completed),
                (Status::Completed, Status::Running),
            ] {
                let mut records = records.clone();
                if reuse {
                    records[2].run.session = records[0].run.session;
                }
                records[0].name = Some("Earlier task".into());
                records[1].name = Some("Other task".into());
                records[2].name = Some("Current task".into());
                records[0].run.status = earlier;
                records[2].run.status = status;
                owner.update(visual, |view, cx| {
                    view.expanded.clear();
                    cx.notify();
                });
                view.update(visual, |view, cx| {
                    view.children = records.clone();
                    view.continuing = false;
                    cx.notify();
                });
                let total = if reuse { 2 } else { 3 };
                let completed = 1
                    + usize::from(status == Status::Completed)
                    + usize::from(!reuse && earlier == Status::Completed);
                let label = View::child_label(completed, total);
                visual.update(|window, cx| {
                    window.render_frame(cx);
                    assert_eq!(
                        window.find(SharedString::from(toggle.clone())).label(),
                        Some(label.as_str())
                    );
                });
                assert!(visual.debug_bounds(list).is_none());
                assert_eq!(
                    visual
                        .debug_bounds(Box::leak(
                            format!("{toggle}-summary-shimmer").into_boxed_str(),
                        ))
                        .is_some(),
                    status == Status::Running || (!reuse && earlier == Status::Queued)
                );
                let selected = if reuse {
                    vec![&records[2], &records[1]]
                } else {
                    records.iter().collect()
                };
                let strip = visual
                    .debug_bounds(Box::leak(
                        format!("live-subagent-icons-{}", key(selected[0])).into_boxed_str(),
                    ))
                    .unwrap();
                assert_eq!(
                    strip.size.width,
                    px(16. * total as f32 + 4. * (total - 1) as f32)
                );
                for (index, child) in records.iter().enumerate() {
                    let id = format!("live-subagent-icon-{}", key(child));
                    let visible = !reuse || index != 0;
                    assert_eq!(
                        visual
                            .debug_bounds(Box::leak(id.clone().into_boxed_str()))
                            .is_some(),
                        visible
                    );
                    for (state, running) in [("rotating", true), ("static", false)] {
                        assert_eq!(
                            visual
                                .debug_bounds(Box::leak(format!("{id}-{state}").into_boxed_str()))
                                .is_some(),
                            visible && (child.run.status == Status::Running) == running
                        );
                    }
                }
                visual.update(|window, cx| window.click(SharedString::from(toggle.clone()), cx));
                visual.run_until_parked();
                visual.update(|window, cx| window.render_frame(cx));
                assert!(visual.debug_bounds(list).is_some());
                let mut previous = None;
                for child in &selected {
                    let id = format!("live-subagent-row-{}", key(child));
                    let row = visual
                        .debug_bounds(Box::leak(id.clone().into_boxed_str()))
                        .unwrap();
                    if let Some(previous) = previous {
                        assert!(row.top() > previous);
                    }
                    previous = Some(row.top());
                    let label = format!(
                        "{}: {}",
                        child.name.as_ref().unwrap(),
                        tr(status_key(child.run.status))
                    );
                    visual.update(|window, _| {
                        assert_eq!(
                            window.find(SharedString::from(id)).label(),
                            Some(label.as_str())
                        );
                    });
                }
                if reuse {
                    assert!(
                        visual
                            .debug_bounds(Box::leak(
                                format!("live-subagent-row-{}", key(&records[0])).into_boxed_str()
                            ))
                            .is_none()
                    );
                }
                assert_eq!(view.read_with(visual, |view, _| view.children.len()), 3);
                view.update(visual, |view, cx| {
                    view.continuing = true;
                    cx.notify();
                });
                visual.update(|window, cx| window.render_frame(cx));
                for (index, child) in records.iter().enumerate() {
                    assert_eq!(
                        visual
                            .debug_bounds(Box::leak(
                                format!("live-subagent-row-{}-summary-shimmer", key(child))
                                    .into_boxed_str(),
                            ))
                            .is_some(),
                        index == 2
                    );
                }
            }
        }
    }
    visual.update(|window, _| window.remove_window());
    drop(view);
    drop(owner);
    fixture.close();
}

#[gpui::test]
fn reused_single_session_stays_a_direct_row(cx: &mut TestAppContext) {
    chat::init(cx);
    let fixture = crate::conversation::live::ChatFixture::with_tools(false, Vec::new());
    let mut records = children(2);
    records[1].run.session = records[0].run.session;
    records[0].name = Some("Earlier task".into());
    records[1].name = Some("Current task".into());
    let first = key(&records[0]);
    let current = format!("live-subagent-row-{}", key(&records[1]));
    let (view, visual) = mount_group(cx, fixture.binding.clone(), records);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        visual.update(|window, cx| {
            Theme::change(mode, Some(window), cx);
            window.render_frame(cx);
            let label = format!("Current task: {}", tr("turn_completed"));
            assert_eq!(
                window.find(SharedString::from(current.clone())).label(),
                Some(label.as_str())
            );
        });
        assert!(
            visual
                .debug_bounds(Box::leak(current.clone().into_boxed_str()))
                .is_some()
        );
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-subagent-row-{first}").into_boxed_str()
                ))
                .is_none()
        );
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-turn-subagents-{first}").into_boxed_str()
                ))
                .is_none()
        );
        assert_eq!(view.read_with(visual, |view, _| view.children.len()), 2);
    }
    visual.update(|window, _| window.remove_window());
    drop(view);
    fixture.close();
}

#[gpui::test]
fn preserves_first_appearance_order(cx: &mut TestAppContext) {
    chat::init(cx);
    let fixture = crate::conversation::live::ChatFixture::with_tools(false, Vec::new());
    let mut records = children(3);
    let mut sessions = records
        .iter()
        .map(|child| child.run.session)
        .collect::<Vec<_>>();
    sessions.sort_unstable();
    for (child, session) in records.iter_mut().zip(sessions.into_iter().rev()) {
        child.run.session = session;
    }
    let first = key(&records[0]);
    let toggle = format!("live-turn-subagents-{first}");
    let (view, visual) = mount_group(cx, fixture.binding.clone(), records.clone());
    let owner = view.read_with(visual, |view, _| view.owner.clone());
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        visual.update(|window, cx| Theme::change(mode, Some(window), cx));
        for sequences in [[1, 1, 1], [9, 3, 1]] {
            for (child, sequence) in records.iter_mut().zip(sequences) {
                child.run.sequence = sequence;
            }
            owner.update(visual, |view, cx| {
                view.expanded.clear();
                cx.notify();
            });
            view.update(visual, |view, cx| {
                view.children = records.clone();
                cx.notify();
            });
            visual.update(|window, cx| window.render_frame(cx));
            let strip = visual
                .debug_bounds(Box::leak(
                    format!("live-subagent-icons-{first}").into_boxed_str(),
                ))
                .unwrap();
            let mut previous = strip.left();
            for child in &records {
                let icon = visual
                    .debug_bounds(Box::leak(
                        format!("live-subagent-icon-{}", key(child)).into_boxed_str(),
                    ))
                    .unwrap();
                assert_eq!(icon.left(), previous);
                assert_eq!(icon.size, size(px(16.), px(16.)));
                previous = icon.right() + px(4.);
            }
            assert_eq!(previous - px(4.), strip.right());
            visual.update(|window, cx| window.click(SharedString::from(toggle.clone()), cx));
            visual.run_until_parked();
            visual.update(|window, cx| window.render_frame(cx));
            let mut previous = None;
            for child in &records {
                let row = visual
                    .debug_bounds(Box::leak(
                        format!("live-subagent-row-{}", key(child)).into_boxed_str(),
                    ))
                    .unwrap();
                if let Some(previous) = previous {
                    assert!(row.top() > previous);
                }
                previous = Some(row.top());
            }
        }
    }
    visual.update(|window, _| window.remove_window());
    drop(view);
    drop(owner);
    fixture.close();
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
