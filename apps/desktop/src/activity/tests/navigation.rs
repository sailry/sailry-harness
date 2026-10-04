use super::*;
use sailry_client::Client;
use sailry_protocol::{
    Command, Output,
    terminal::{self, Input, Launch, Viewport},
};

#[gpui::test]
fn terminal_targets(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let fixture = fixture::Fixture::new();
    let terminal_home = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(fixture.services());
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        entity = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = entity.unwrap();
    wait(visual, |cx| {
        shell.read(cx).activity.observers.len() == 2
            && shell
                .read(cx)
                .activity
                .observers
                .values()
                .all(|observer| observer.view.connected)
    });
    for index in 0..2 {
        let node = fixture.nodes[index].id();
        let transport = if index == 0 {
            fixture.nodes[0].local()
        } else {
            fixture.nodes[0]
                .link()
                .remote(fixture.nodes[1].link().address())
        };
        let client = Client::new(transport);
        #[cfg(unix)]
        fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::SaveTerminalSettings(
                terminal::Settings {
                    shell: "/bin/sh".into(),
                    environment:
                        [("HOME".into(), terminal_home.path().to_str().unwrap().into())].into(),
                    ..Default::default()
                },
            ))))
            .unwrap();
        let appearance = visual.update(|_, cx| crate::theme::terminal(cx));
        let Output::Terminal(info) = fixture
            .runtime
            .block_on(
                client.execute(client.prepare(Command::CreateTerminal(Launch {
                    worktree: fixture.sessions[index].worktree,
                    viewport: Viewport {
                        columns: 80,
                        rows: 24,
                        pixel_width: 0,
                        pixel_height: 0,
                    },
                    appearance,
                }))),
            )
            .unwrap()
        else {
            panic!("terminal expected")
        };
        wait(visual, |cx| {
            shell
                .read(cx)
                .activity
                .snapshot(node)
                .is_some_and(|snapshot| snapshot.terminals.iter().any(|entry| entry.id == info.id))
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
            fixture
                .runtime
                .block_on(client.execute(client.prepare(Command::InputTerminal {
                    terminal: info.id,
                    revision: info.revision,
                    input,
                })))
                .unwrap();
        }
        wait(visual, |cx| {
            shell
                .read(cx)
                .activity
                .inbox
                .notices()
                .iter()
                .any(|notice| {
                    notice.id.node == node
                        && notice.id.target == Target::Terminal(info.id)
                        && notice.kind == Kind::Completed
                })
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .live
                    .as_mut()
                    .unwrap()
                    .select(fixture.nodes[1 - index].id(), cx);
                shell.navigate(Page::Settings, window, cx);
            });
            window.clear_notifications(cx);
        });
        let row = shell.read_with(visual, |shell, _| {
            shell
                .activity
                .inbox
                .notices()
                .iter()
                .position(|notice| {
                    notice.id.node == node && notice.id.target == Target::Terminal(info.id)
                })
                .unwrap()
        });
        click(visual, "notifications-open");
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("notification-{row}-icon").into_boxed_str()
                ))
                .is_some()
        );
        click(
            visual,
            Box::leak(format!("notification-{row}").into_boxed_str()),
        );
        wait(visual, |cx| {
            let shell = shell.read(cx);
            shell.page == Page::Terminal
                && shell.splits.read(cx).active
                    == Some(crate::panes::Target::Terminal(
                        node,
                        info.worktree.unwrap(),
                        info.id,
                    ))
        });
        assert!(!shell.read_with(visual, |shell, _| shell.activity.open));
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.live.as_ref().unwrap().selected),
            node
        );
        assert!(shell.read_with(visual, |shell, _| {
            shell.activity.unread_terminals(node).is_empty()
        }));
    }
    visual.update(|window, _| window.remove_window());
    drop(shell);
    fixture.close();
}
