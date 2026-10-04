use super::workspace::click;
use super::*;
use crate::conversation::{fixture, queue::Target, turn::Status};
use std::time::Duration;

fn frame(cx: &mut VisualTestContext, elapsed: u64) {
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_millis(elapsed));
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        _ = window.draw(cx);
    });
}

fn waiting(shell: &Entity<Shell>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_session((0, 2), window, cx);
            let thread = shell.conversations.get_mut(&(0, 2)).unwrap();
            thread.turns.push(fixture::sample(2));
            thread.scroller.update(cx, |state, cx| state.reset(1, cx));
            cx.notify();
        });
    });
    frame(cx, 0);
}

fn draft(shell: &Entity<Shell>, cx: &mut VisualTestContext, text: &str) {
    cx.update(|window, cx| {
        shell.read(cx).conversations[&(0, 2)]
            .input
            .clone()
            .update(cx, |input, cx| {
                input.set_value(text.to_owned(), window, cx);
                input.focus(window, cx);
            });
    });
    frame(cx, 0);
}

fn enqueue(shell: &Entity<Shell>, cx: &mut VisualTestContext, text: &str) {
    draft(shell, cx, text);
    cx.simulate_keystrokes("enter");
    frame(cx, 0);
}

fn target(id: usize) -> Target {
    Target {
        session: (0, 2),
        id,
        revision: 0,
    }
}

#[gpui::test]
fn fifo_frozen_options(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.select_session((0, 2), window, cx)));
    enqueue(&shell, &mut cx, "First");
    enqueue(&shell, &mut cx, "Second");
    cx.update(|_, cx| {
        shell.update(cx, |shell, _| {
            let thread = shell.conversations.get_mut(&(0, 2)).unwrap();
            assert_eq!(thread.turns.len(), 1);
            assert_eq!(thread.queue.entries.len(), 1);
            assert!(thread.queue.entries[0].options.model.is_some());
            assert!(thread.queue.entries[0].options.effort.is_some());
            thread.options.choices[0] = 1;
            thread.options.attachment = true;
        });
    });
    enqueue(&shell, &mut cx, "");
    draft(&shell, &mut cx, "Unsent draft");
    for expected in [2, 3] {
        frame(&mut cx, 8500);
        cx.update(|_, cx| {
            let thread = &shell.read(cx).conversations[&(0, 2)];
            assert_eq!(thread.turns.len(), expected);
            assert_eq!(thread.turns[expected - 2].status, Status::Completed);
            assert_eq!(thread.input.read(cx).value(), "Unsent draft");
        });
    }
    frame(&mut cx, 8500);
    cx.update(|_, cx| {
        let thread = &shell.read(cx).conversations[&(0, 2)];
        assert_eq!(thread.turns[1].prompt, "Second");
        assert_eq!(thread.turns[1].options.as_ref().unwrap().choices[0], 0);
        assert!(thread.turns[2].prompt.is_empty());
        assert!(thread.turns[2].options.as_ref().unwrap().attachment);
        assert_eq!(thread.turns[2].options.as_ref().unwrap().choices[0], 1);
        assert!(thread.queue.entries.is_empty());
        assert!(!thread.options.attachment);
        assert_eq!(thread.scroller.read(cx).item_count(), 3);
    });
}

#[gpui::test]
fn scopes_stale_actions(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    waiting(&shell, &mut cx);
    for text in ["First", "Second", "Third"] {
        enqueue(&shell, &mut cx, text);
    }
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let options = shell.conversations[&(0, 2)].queue.entries[0]
                .options
                .clone();
            assert!(!shell.edit_queued(target(0), "  ".into(), cx));
            assert!(shell.edit_queued(target(0), "Edited".into(), cx));
            assert!(!shell.edit_queued(target(0), "Stale".into(), cx));
            shell.remove_queued(target(0), cx);
            shell.send_queued(target(0), window, cx);
            shell.reorder_queued(target(0), target(2), cx);
            assert_eq!(
                shell.conversations[&(0, 2)].turns[0].status,
                Status::Waiting
            );
            let revised = Target {
                revision: 1,
                ..target(0)
            };
            shell.reorder_queued(
                revised,
                Target {
                    session: (1, 2),
                    ..target(2)
                },
                cx,
            );
            shell.reorder_queued(revised, target(2), cx);
            let entries = &shell.conversations[&(0, 2)].queue.entries;
            assert_eq!(
                entries.iter().map(|entry| entry.id).collect::<Vec<_>>(),
                [1, 2, 0]
            );
            assert_eq!(entries[2].options, options);
            shell.remove_queued(revised, cx);
            shell.remove_queued(revised, cx);
            assert_eq!(shell.conversations[&(0, 2)].queue.entries.len(), 2);
        });
    });
    enqueue(&shell, &mut cx, "Fourth");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)]
            .queue
            .entries
            .last()
            .unwrap()
            .id),
        3
    );
}

#[gpui::test]
fn pause_and_send_now(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.select_session((0, 2), window, cx)));
    enqueue(&shell, &mut cx, "First");
    frame(&mut cx, 6000);
    let partial = cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)].turns[0].copy_text());
    enqueue(&shell, &mut cx, "Second");
    enqueue(&shell, &mut cx, "Third");
    draft(&shell, &mut cx, "Keep draft");
    click(&mut cx, "composer-send");
    frame(&mut cx, 10000);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let thread = &shell.conversations[&(0, 2)];
            assert_eq!(thread.turns.len(), 1);
            assert_eq!(thread.turns[0].status, Status::Cancelled);
            assert_eq!(thread.turns[0].copy_text(), partial);
            assert_eq!(thread.queue.entries.len(), 2);
            assert!(thread.queue.paused);
            shell.send_queued(target(2), window, cx);
            shell.send_queued(target(2), window, cx);
            let thread = &shell.conversations[&(0, 2)];
            assert_eq!(thread.turns.len(), 2);
            assert_eq!(thread.turns[1].prompt, "Third");
            assert_eq!(thread.queue.entries[0].text, "Second");
            assert_eq!(thread.input.read(cx).value(), "Keep draft");
        });
    });
}

#[gpui::test]
fn rejects_changed_authority(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    waiting(&shell, &mut cx);
    enqueue(&shell, &mut cx, "Local queue");
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let owner = shell.workspace.sessions[&(0, 2)].owner;
            shell
                .workspace
                .sessions
                .get_mut(&(0, 2))
                .unwrap()
                .owner
                .worktree += 100;
            shell.send_queued(target(0), window, cx);
            assert_eq!(
                shell.conversations[&(0, 2)].turns[0].status,
                Status::Waiting
            );
            shell.workspace.sessions.get_mut(&(0, 2)).unwrap().owner = owner;
            shell
                .workspace
                .projects
                .get_mut(&owner.project)
                .unwrap()
                .trusted = false;
            shell.send_queued(target(0), window, cx);
            assert_eq!(
                shell.conversations[&(0, 2)].turns[0].status,
                Status::Waiting
            );
            assert_eq!(shell.conversations[&(0, 2)].queue.entries.len(), 1);
            shell.select_composer_host(1, window, cx);
            assert!(shell.conversations[&(1, 2)].queue.entries.is_empty());
            shell.send_queued(
                Target {
                    session: (1, 2),
                    ..target(0)
                },
                window,
                cx,
            );
            assert!(shell.conversations[&(1, 2)].turns.is_empty());
        });
    });
}

#[gpui::test]
fn popup_actions(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    waiting(&shell, &mut cx);
    for text in ["First", "Second", "Third"] {
        enqueue(&shell, &mut cx, text);
    }
    draft(&shell, &mut cx, "Unsent draft");
    click(&mut cx, "composer-queue");
    let trigger = cx.debug_bounds("composer-queue").unwrap();
    let panel = cx.debug_bounds("queue-panel").unwrap();
    assert!(panel.bottom() < trigger.top());
    click(&mut cx, "queue-edit-0");
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("Edited");
    cx.simulate_keystrokes("enter");
    frame(&mut cx, 0);
    assert!(cx.debug_bounds("queue-input").is_none());
    assert_eq!(
        cx.update(
            |_, cx| shell.read(cx).conversations[&(0, 2)].queue.entries[0]
                .text
                .clone()
        ),
        "Edited"
    );
    for (row, actions) in [
        (
            "queue-row-0",
            ["queue-edit-0", "queue-send-0", "queue-delete-0"],
        ),
        (
            "queue-row-1",
            ["queue-edit-1", "queue-send-1", "queue-delete-1"],
        ),
        (
            "queue-row-2",
            ["queue-edit-2", "queue-send-2", "queue-delete-2"],
        ),
    ] {
        let row = cx.debug_bounds(row).unwrap();
        for action in actions {
            let button = cx.debug_bounds(action).unwrap();
            assert!(button.top() >= row.top() && button.bottom() <= row.bottom());
            assert!(button.left() >= row.left() && button.right() <= row.right());
        }
    }
    click(&mut cx, "queue-edit-1");
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("Discard");
    frame(&mut cx, 0);
    click(&mut cx, "queue-edit-cancel");
    let from = cx.debug_bounds("queue-drag-0").unwrap().center();
    let to = cx.debug_bounds("queue-row-2").unwrap().center();
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(
        from + point(px(8.), px(8.)),
        MouseButton::Left,
        Modifiers::default(),
    );
    frame(&mut cx, 0);
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
    frame(&mut cx, 0);
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
    frame(&mut cx, 0);
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)]
            .queue
            .entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>()),
        [1, 2, 0]
    );
    click(&mut cx, "queue-delete-1");
    assert!(cx.debug_bounds("queue-row-1").is_none());
    cx.simulate_keystrokes("escape");
    frame(&mut cx, 0);
    assert!(cx.debug_bounds("queue-panel").is_none());
    cx.simulate_keystrokes("secondary-right");
    cx.simulate_input(" retained");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)].input.read(cx).value()),
        "Unsent draft retained"
    );
}

#[gpui::test]
fn geometry_and_approval_coexistence(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_session((0, 2), window, cx);
            shell.show_approval_preview((0, 2), cx);
        })
    });
    for _ in 0..12 {
        enqueue(
            &shell,
            &mut cx,
            "Long queued preview message without wrapping into additional composer rows",
        );
    }
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [760., 1280.] {
            let handle = cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(width), px(820.)));
            frame(&mut cx, 400);
            assert!(cx.debug_bounds("pending-approvals").is_some());
            click(&mut cx, "composer-queue");
            let panel = cx.debug_bounds("queue-panel").unwrap();
            assert!(panel.size.height <= px(280.));
            assert!(panel.left() >= px(0.) && panel.right() <= px(width));
            assert!(panel.bottom() < cx.debug_bounds("composer-queue").unwrap().top());
            cx.simulate_keystrokes("escape");
            frame(&mut cx, 0);
        }
    }
    click(&mut cx, "approval-approve-0");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)].turns.len()),
        1
    );
    click(&mut cx, "approval-reject-1");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)].turns.len()),
        2
    );
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)].queue.entries.len()),
        11
    );
}

#[gpui::test]
fn empty_entry_actions(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    waiting(&shell, &mut cx);
    draft(&shell, &mut cx, "Only message");
    click(&mut cx, "composer-enqueue");
    click(&mut cx, "composer-queue");
    click(&mut cx, "queue-edit-0");
    cx.simulate_keystrokes("secondary-a backspace");
    frame(&mut cx, 0);
    click(&mut cx, "queue-edit-save");
    assert!(cx.debug_bounds("queue-input").is_some());
    cx.simulate_keystrokes("escape");
    frame(&mut cx, 0);
    click(&mut cx, "composer-queue");
    assert!(cx.debug_bounds("queue-input").is_none());
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            let owner = shell.workspace.sessions[&(0, 2)].owner;
            shell
                .workspace
                .projects
                .get_mut(&owner.project)
                .unwrap()
                .trusted = false;
            cx.notify();
        })
    });
    frame(&mut cx, 0);
    click(&mut cx, "queue-send-0");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)].turns[0].status),
        Status::Waiting
    );
    assert!(cx.debug_bounds("queue-panel").is_some());
    click(&mut cx, "queue-delete-0");
    assert!(cx.debug_bounds("queue-panel").is_none());
    assert!(cx.debug_bounds("composer-queue").is_none());
}

#[gpui::test]
fn drain_preserves_draft(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_session((0, 2), window, cx);
            shell.show_interaction_preview((0, 2), fixture::InteractionScene::Single, window, cx);
        })
    });
    enqueue(&shell, &mut cx, "Queued after answer");
    draft(&shell, &mut cx, "Keep next draft");
    assert!(cx.debug_bounds("pending-interaction").is_some());
    assert!(cx.debug_bounds("composer-queue").is_some());
    click(&mut cx, "question-choice-0");
    click(&mut cx, "interaction-submit");
    cx.update(|_, cx| {
        let thread = &shell.read(cx).conversations[&(0, 2)];
        assert_eq!(thread.turns.len(), 2);
        assert_eq!(thread.turns[0].status, Status::Completed);
        assert_eq!(thread.turns[1].prompt, "Queued after answer");
        assert!(thread.queue.entries.is_empty());
        assert!(thread.interaction_drafts.is_empty());
        assert_eq!(thread.input.read(cx).value(), "Keep next draft");
    });
}
