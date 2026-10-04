use super::*;
use core::prelude::v1::test;
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::Output;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
mod cursor;
mod effects;
mod keyboard;
mod links;

struct Mounted(Entity<View>);
impl Render for Mounted {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

#[track_caller]
fn wait(cx: &mut VisualTestContext, view: &Entity<View>, predicate: impl Fn(&View) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        if view.read_with(cx, |view, _| predicate(view)) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "terminal view update deadline: {:?}",
            view.read_with(cx, |view, _| (
                view.message,
                view.state.snapshot.as_ref().map(|snapshot| (
                    snapshot.screen.alternate,
                    snapshot.screen.mouse_tracking,
                    snapshot.info.revision,
                )),
                content(view),
            ))
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn content(view: &View) -> String {
    view.state
        .snapshot
        .iter()
        .flat_map(|snapshot| {
            snapshot
                .screen
                .scrollback
                .iter()
                .chain(&snapshot.screen.rows)
        })
        .flat_map(|line| line.spans.iter().map(|span| span.text.as_str()))
        .collect()
}

fn paste(cx: &mut VisualTestContext, text: &str) {
    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string(text.into())));
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-v"
    } else {
        "ctrl-shift-v"
    });
}

#[gpui::test]
#[cfg(unix)]
fn input_and_remount(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    let local = runtime
        .block_on(Node::start(directory.path().join("local")))
        .unwrap();
    let remote = runtime
        .block_on(Node::start(directory.path().join("remote")))
        .unwrap();
    let address = runtime
        .block_on(local.link().pair(remote.link().invite().unwrap().ticket()))
        .unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shortcuts::init(cx);
        super::init(cx);
    });
    for (transport, controller) in [
        (
            local.local() as Arc<dyn sailry_link::Transport>,
            remote.link().remote(local.link().address()),
        ),
        (
            local.link().remote(address),
            remote.local() as Arc<dyn sailry_link::Transport>,
        ),
    ] {
        let client = Arc::new(Client::new(transport));
        let controller = Client::new(controller);
        runtime
            .block_on(client.execute(client.prepare(Command::RegisterProject {
                name: "Terminal fixture".into(),
                path: directory.path().to_str().unwrap().into(),
            })))
            .unwrap();
        let Output::Snapshot(snapshot) = runtime
            .block_on(client.execute(client.prepare(Command::Snapshot)))
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let worktree = snapshot.worktrees[0].id;
        let appearance = cx.read(crate::theme::terminal);
        let Output::Terminal(info) = runtime
            .block_on(
                controller.execute(
                    controller.prepare(Command::CreateTerminal(protocol::Launch {
                        worktree,
                        viewport: protocol::Viewport {
                            columns: 80,
                            rows: 24,
                            pixel_width: 640,
                            pixel_height: 384,
                        },
                        appearance,
                    })),
                ),
            )
            .unwrap()
        else {
            panic!("terminal expected")
        };
        let binding = Binding {
            scope: None,
            client: client.clone(),
            caller: local.id(),
            id: info.id,
            runtime: runtime.handle().clone(),
        };
        let mut entity = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| View::new(binding.clone(), window, cx));
            view.update(cx, |view, cx| view.focus(window, cx));
            entity = Some(view.clone());
            let mounted = cx.new(|_| Mounted(view));
            Root::new(mounted, window, cx)
        });
        let view = entity.unwrap();
        wait(visual, &view, |view| {
            view.state.connected && view.state.snapshot.is_some()
        });
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.activate(window, cx);
                assert!(
                    !view.claim_pending,
                    "focus must preserve another controller"
                );
                assert!(!view.controlling());
                assert_eq!(view.info().unwrap().owner, Some(remote.id()));
                assert_eq!(view.info().unwrap().revision, info.revision);
            });
        });
        let Output::TerminalSnapshot(snapshot) = runtime
            .block_on(
                client.execute(client.prepare(Command::InspectTerminal { terminal: info.id })),
            )
            .unwrap()
        else {
            panic!("terminal snapshot expected")
        };
        assert_eq!(snapshot.info.owner, Some(remote.id()));
        assert_eq!(snapshot.info.revision, info.revision);
        let takeover = visual.debug_bounds("terminal-take-control").unwrap();
        visual.simulate_click(takeover.center(), Modifiers::default());
        wait(visual, &view, View::controlling);
        assert!(view.read_with(visual, |view, _| {
            view.info().unwrap().revision > info.revision
        }));
        view.update(visual, |view, _| {
            let original = view.state.clone();
            let mut animated = original.clone();
            let snapshot = Arc::make_mut(animated.snapshot.as_mut().unwrap());
            snapshot.sequence += 1;
            snapshot.screen.rows[0].spans.clear();
            view.cursor_visible = false;
            view.update_screen(animated.clone());
            assert!(
                !view.cursor_visible,
                "background output restarted the cursor blink"
            );
            let snapshot = Arc::make_mut(animated.snapshot.as_mut().unwrap());
            snapshot.screen.cursor = Some(protocol::Cursor {
                column: 3,
                row: 2,
                at_wide_tail: false,
                style: protocol::CursorStyle::Bar,
                blinking: true,
            });
            view.update_screen(animated);
            assert!(view.cursor_visible, "cursor movement must reveal the caret");
            view.update_screen(original);
        });
        visual.update(|window, _| window.activate_window());
        visual.run_until_parked();
        let grid = visual.debug_bounds("terminal-grid").unwrap();
        let original_text = view.read_with(visual, |view, _| content(view));
        visual.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.message = Some("terminal_input_unknown");
                cx.notify();
            });
        });
        crate::feedback::tests::shown(visual);
        crate::feedback::tests::settle(visual);
        assert_eq!(
            visual.update(crate::feedback::tests::summary),
            tr("terminal_input_unknown")
        );
        assert!(visual.debug_bounds("terminal-status").is_none());
        assert_eq!(visual.debug_bounds("terminal-grid").unwrap(), grid);
        assert_eq!(
            view.read_with(visual, |view, _| content(view)),
            original_text
        );
        let original = visual.update(|window, cx| window.notifications(cx));
        visual.update(|_, cx| view.update(cx, |_, cx| cx.notify()));
        visual.run_until_parked();
        assert_eq!(
            visual.update(|window, cx| window.notifications(cx)),
            original
        );
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.message = None;
                cx.notify();
            });
            window.clear_notifications(cx);
        });
        crate::feedback::tests::settle(visual);
        paste(visual, &"x".repeat(protocol::MAX_INPUT_BYTES + 1));
        crate::feedback::tests::shown(visual);
        assert_eq!(
            visual.update(crate::feedback::tests::summary),
            tr("terminal_paste_too_large")
        );
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("terminal-status").is_none());
        assert_eq!(
            view.read_with(visual, |view, _| content(view)),
            original_text
        );
        view.update(visual, |view, cx| {
            view.state.connected = false;
            view.input(
                protocol::Input::Paste {
                    text: "not sent".into(),
                },
                cx,
            );
            assert_eq!(view.message, None);
            assert_eq!(view.status(), Some("terminal_connecting"));
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("terminal-status").is_some());
        assert_eq!(visual.debug_bounds("terminal-grid").unwrap(), grid);
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.state.connected = true;
                assert_eq!(view.status(), None);
                let original = view.state.snapshot.clone();
                let snapshot = Arc::make_mut(view.state.snapshot.as_mut().unwrap());
                snapshot.info.ssh = Some(sailry_protocol::SshId::new());
                snapshot.info.owner = Some(sailry_protocol::NodeId([7; 32]));
                view.activate(window, cx);
                assert!(
                    !view.claim_pending,
                    "SSH focus must not claim another shell"
                );
                assert_eq!(view.status(), Some("terminal_unavailable"));
                view.state.snapshot = original;
                cx.notify();
            })
        });
        assert_eq!(
            view.read_with(visual, |view, _| (
                view.binding.client.target(),
                view.binding.id
            )),
            (client.target(), info.id)
        );
        let handle = visual.update(|window, _| window.window_handle());
        // A visible terminal still owns its viewport while another split has focus.
        let other = visual.update(|window, cx| {
            let focus = cx.focus_handle();
            focus.focus(window, cx);
            focus
        });
        visual.simulate_window_resize(handle, size(px(620.), px(420.)));
        wait(visual, &view, |view| {
            view.state.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.screen.columns == view.metrics.columns
                    && snapshot.screen.rows.len() == usize::from(view.metrics.rows)
            })
        });
        visual.update(|window, cx| {
            assert!(other.is_focused(window));
            view.update(cx, |view, cx| view.focus(window, cx));
        });
        visual.simulate_input("printf 'terminal-%s\\n' ready");
        visual.simulate_keystrokes("enter");
        wait(visual, &view, |view| {
            content(view).contains("terminal-ready")
        });
        // Marked text stays in GPUI until the input method commits it.
        let before = view.read_with(visual, |view, _| content(view));
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.replace_and_mark_text_in_range(None, "中文🙂", Some(4..4), window, cx);
                assert_eq!(view.composition.text, "中文🙂");
                assert_eq!(content(view), before);
                view.unmark_text(window, cx);
            })
        });
        visual.simulate_input("printf '%s\\n' '中文🙂'");
        visual.simulate_keystrokes("enter");
        wait(visual, &view, |view| content(view).contains("中文🙂"));
        paste(visual, "printf 'clipboard-%s\\n' ready");
        visual.simulate_keystrokes("enter");
        wait(visual, &view, |view| {
            content(view).contains("clipboard-ready")
        });
        let columns = view.read_with(visual, |view, _| view.metrics.columns);
        visual.update(|_, cx| {
            crate::preferences::update(cx, |data| {
                data.terminal.font_size = 22;
                data.terminal.paste_protection = true;
            })
        });
        wait(visual, &view, |view| view.metrics.columns < columns);
        paste(visual, "printf 'paste-%s\\n' confirmed\n");
        assert!(visual.has_pending_prompt());
        assert!(!view.read_with(visual, |view, _| content(view).contains("paste-confirmed")));
        crate::prompts::tests::answer(visual, "terminal_paste");
        visual.simulate_keystrokes("enter");
        wait(visual, &view, |view| {
            content(view).contains("paste-confirmed")
        });
        visual.update(|_, cx| crate::preferences::update(cx, |data| data.terminal.font_size = 14));
        visual.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a cmd-c"
        } else {
            "ctrl-shift-a ctrl-shift-c"
        });
        visual.update(|_, cx| {
            assert!(
                cx.read_from_clipboard()
                    .unwrap()
                    .text()
                    .unwrap()
                    .contains("terminal-ready")
            )
        });
        visual.dispatch_action(gpui_kit::component::input::SelectAll);
        visual.dispatch_action(gpui_kit::component::input::Copy);
        visual.run_until_parked();
        assert!(
            visual
                .read_from_clipboard()
                .unwrap()
                .text()
                .unwrap()
                .contains("terminal-ready")
        );
        visual.update(|_, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(
                "printf 'native-menu-paste\\n'\n".into(),
            ))
        });
        visual.dispatch_action(gpui_kit::component::input::Paste);
        visual.run_until_parked();
        assert!(visual.has_pending_prompt());
        crate::prompts::tests::answer(visual, "terminal_paste");
        visual.simulate_keystrokes("enter");
        wait(visual, &view, |view| {
            content(view).contains("native-menu-paste")
        });
        visual.update(|window, cx| Theme::change(ThemeMode::Light, Some(window), cx));
        let appearance = visual.read(crate::theme::terminal);
        wait(visual, &view, |view| {
            view.state
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.screen.background == appearance.background)
        });
        visual.update(|window, cx| Theme::change(ThemeMode::Dark, Some(window), cx));
        let appearance = visual.read(crate::theme::terminal);
        wait(visual, &view, |view| {
            view.state
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.screen.background == appearance.background)
        });
        visual.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-f"
        } else {
            "ctrl-f"
        });
        wait(visual, &view, |view| view.search.open);
        visual.simulate_input("terminal-ready");
        wait(visual, &view, |view| !view.search.matches.is_empty());
        visual.simulate_keystrokes("escape");
        wait(visual, &view, |view| !view.search.open);
        keyboard::check(visual, &view, directory.path());
        effects::check(visual, &view, directory.path());
        links::check(visual, &view, directory.path());
        cursor::check(visual, &view);
        // A real editor exercises alternate screen, mouse positioning, and process input.
        // Paste the script as one event; simulated typing emits an unpaced burst.
        paste(
            visual,
            "printf 'first\\nsecond\\nthird\\n' > tui.txt; vim -Nu NONE -i NONE -n --cmd 'set mouse=a' tui.txt",
        );
        visual.simulate_keystrokes("enter");
        assert_eq!(
            view.read_with(visual, |view, _| view.message),
            None,
            "editor input must be admitted"
        );
        wait(visual, &view, |view| {
            view.state
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.screen.alternate && snapshot.screen.mouse_tracking)
        });
        let position = view.read_with(visual, |view, _| {
            point(
                view.metrics.bounds.left() + view.metrics.cell.width,
                view.metrics.bounds.top() + view.metrics.cell.height * 1.5,
            )
        });
        visual.simulate_click(position, Modifiers::default());
        wait(visual, &view, |view| {
            view.state
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.screen.cursor)
                .is_some_and(|cursor| cursor.row == 1)
        });
        visual.simulate_input("Iedited-");
        visual.simulate_keystrokes("escape");
        visual.simulate_input(":wq");
        visual.simulate_keystrokes("enter");
        wait(visual, &view, |view| {
            view.state
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| !snapshot.screen.alternate)
        });
        assert_eq!(
            std::fs::read_to_string(directory.path().join("tui.txt")).unwrap(),
            "first\nedited-second\nthird\n"
        );
        visual.update(|window, _| window.remove_window());
        drop(view);
        let Output::TerminalSnapshot(snapshot) = runtime
            .block_on(
                client.execute(client.prepare(Command::InspectTerminal { terminal: info.id })),
            )
            .unwrap()
        else {
            panic!("terminal snapshot expected")
        };
        assert_eq!(snapshot.info.status, protocol::Status::Running);
        let mut entity = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| View::new(binding, window, cx));
            entity = Some(view.clone());
            Root::new(view, window, cx)
        });
        let view = entity.unwrap();
        wait(visual, &view, |view| {
            content(view).contains("terminal-ready")
        });
        runtime
            .block_on(client.execute(client.prepare(Command::CloseTerminal {
                worktree,
                terminal: info.id,
            })))
            .unwrap();
        wait(visual, &view, |view| {
            view.state
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.info.status == protocol::Status::Closed)
        });
        visual.update(|window, _| window.remove_window());
    }
    runtime.block_on(local.shutdown()).unwrap();
    runtime.block_on(remote.shutdown()).unwrap();
}

#[test]
fn selection_boundaries() {
    use selection::{Position, Selection};
    let line = |text: &str, columns, wrapped| protocol::Line {
        spans: vec![protocol::Span {
            column: 0,
            columns,
            text: text.into(),
            style: Default::default(),
            hyperlink: None,
        }],
        wrapped,
    };
    let color = protocol::Rgb {
        red: 0,
        green: 0,
        blue: 0,
    };
    let screen = protocol::Screen {
        columns: 4,
        scrollback: vec![line("abcd", 4, true)],
        rows: vec![line("ef", 2, false), line("中", 2, false)],
        cursor: None,
        foreground: color,
        background: color,
        cursor_color: color,
        bracketed_paste: false,
        mouse_tracking: false,
        alternate: false,
        features: Default::default(),
        graphics: Default::default(),
    };
    let selection = Selection {
        anchor: Some(Position { row: 0, column: 1 }),
        end: Some(Position { row: 2, column: 1 }),
        dragging: false,
        ..Default::default()
    };
    assert_eq!(selection.text(&screen), "bcdef\n中");
    assert_eq!(
        super::text::find(&screen, "cdef"),
        vec![(
            Position { row: 0, column: 2 },
            Position { row: 1, column: 2 }
        )]
    );
    assert_eq!(
        super::text::find(&screen, "中"),
        vec![(
            Position { row: 2, column: 0 },
            Position { row: 2, column: 2 }
        )]
    );
    assert_eq!(
        super::selection::extent(&screen, Position { row: 0, column: 2 }, true),
        (
            Position { row: 0, column: 0 },
            Position { row: 1, column: 2 }
        )
    );
    assert_eq!(
        super::selection::extent(&screen, Position { row: 0, column: 2 }, false),
        (
            Position { row: 0, column: 0 },
            Position { row: 1, column: 4 }
        )
    );
    let rectangular = Selection {
        anchor: Some(Position { row: 1, column: 2 }),
        end: Some(Position { row: 0, column: 0 }),
        rectangular: true,
        ..Default::default()
    };
    assert_eq!(rectangular.text(&screen), "ab\nef");
}
