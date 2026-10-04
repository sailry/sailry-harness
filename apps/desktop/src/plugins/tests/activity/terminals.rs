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
        let completed = format!("activity-project-activity_completed-{project}");
        wait(visual, |cx| snapshot(&panel, cx).contains(&completed));
        let selector: &'static str =
            Box::leak(format!("activity-terminal-{}", info.id).into_boxed_str());
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let card = visual.debug_bounds(selector).unwrap();
        let column = visual.debug_bounds("activity_completed-column").unwrap();
        assert!(card.left() >= column.left() && card.right() <= column.right());
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
            snapshot(&panel, cx).contains(&format!("activity-project-activity_idle-{project}"))
                && shell
                    .read(cx)
                    .activity
                    .unread_terminals(fixture.node.id())
                    .is_empty()
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let card = visual.debug_bounds(selector).unwrap();
        let column = visual.debug_bounds("activity_idle-column").unwrap();
        assert!(card.left() >= column.left() && card.right() <= column.right());
        let header: &'static str =
            Box::leak(format!("activity-project-activity_idle-{project}").into_boxed_str());
        click(visual, header);
        assert!(visual.debug_bounds(selector).is_none());
        click(visual, header);
        click(visual, selector);
        wait(visual, |cx| shell.read(cx).page == Page::Terminal);
        visual.update(|window, _| window.remove_window());
        drop(cached);
        drop(panel);
        drop(shell);
        fixture.close();
    }
}
