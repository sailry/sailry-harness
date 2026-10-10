#![cfg(unix)]
use super::*;
use sailry_protocol::terminal::{self, Input, Launch, Viewport};

#[gpui::test]
fn unread_result_opens_captured_terminal(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let (shell, visual) = files::mount(&fixture, remote, cx);
        let panel = open(&shell, &fixture, visual);
        let directory = tempfile::tempdir().unwrap();
        fixture.execute(Command::SaveTerminalSettings(terminal::Settings {
            shell: "/bin/sh".into(),
            environment: [("HOME".into(), directory.path().to_str().unwrap().into())].into(),
            ..Default::default()
        }));
        let appearance = visual.update(|_, cx| crate::theme::terminal(cx));
        let Output::Terminal(info) = fixture.execute(Command::CreateTerminal(Launch {
            access: sailry_protocol::process::Access::Workspace,
            worktree: fixture.session.worktree,
            viewport: Viewport {
                columns: 80,
                rows: 24,
                pixel_width: 0,
                pixel_height: 0,
            },
            appearance,
        })) else {
            panic!("terminal expected");
        };
        wait(visual, |cx| {
            shell
                .read(cx)
                .activity
                .snapshot(fixture.node.id())
                .is_some_and(|snapshot| {
                    snapshot
                        .terminals
                        .iter()
                        .any(|terminal| terminal.id == info.id)
                })
        });
        for input in [
            Input::Paste {
                text: "exit 0".into(),
            },
            Input::Key {
                event: terminal::KeyEvent {
                    key: terminal::Key::Enter,
                    action: terminal::Action::Press,
                    modifiers: Default::default(),
                    utf8: None,
                    unshifted_codepoint: None,
                },
            },
        ] {
            fixture.execute(Command::InputTerminal {
                terminal: info.id,
                revision: info.revision,
                input,
            });
        }
        let project = fixture.session.project.unwrap();
        wait(visual, |cx| {
            snapshot(&panel, cx).contains(&format!("activity-terminal-{}", info.id))
                && shell
                    .read(cx)
                    .activity
                    .unread_terminals(fixture.node.id())
                    .contains(&info.id)
        });
        let selector: &'static str =
            Box::leak(format!("activity-terminal-{}", info.id).into_boxed_str());
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let person = visual.debug_bounds(selector).unwrap();
        let room = visual
            .debug_bounds(super::selector(format!("activity-room-tile-{project}")))
            .unwrap();
        assert!(room.contains(&person.center()));
        click(visual, selector);
        wait(visual, |cx| {
            shell.read(cx).page == Page::Terminal
                && shell.read(cx).splits.read(cx).active
                    == Some(crate::panes::Target::Terminal(
                        fixture.node.id(),
                        info.worktree.unwrap(),
                        info.id,
                    ))
        });
        let cached = open(&shell, &fixture, visual);
        assert_eq!(cached, panel);
        wait(visual, |cx| {
            shell
                .read(cx)
                .activity
                .unread_terminals(fixture.node.id())
                .is_empty()
        });
        click(visual, super::selector(format!("activity-room-{project}")));
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("activity-detail-list")
        });
        click(
            visual,
            super::selector(format!("activity-detail-terminal-{}", info.id)),
        );
        wait(visual, |cx| shell.read(cx).page == Page::Terminal);
        visual.update(|window, _| window.remove_window());
        drop(cached);
        drop(panel);
        drop(shell);
        fixture.close();
    }
}
