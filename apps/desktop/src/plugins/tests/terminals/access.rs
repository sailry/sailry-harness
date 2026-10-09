use super::*;
use sailry_protocol::{process::Access, terminal::Tool};
use std::os::unix::fs::PermissionsExt;

#[gpui::test]
fn requires_explicit_full_terminal_choice(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = fixture(remote);
        let (panel, visual) = open(&fixture, cx);
        assert!(terminals(&fixture).is_empty());
        click(visual, "terminal-new");
        wait(visual, |_| terminals(&fixture).len() == 1);
        let workspace = terminals(&fixture).remove(0);
        assert_eq!(workspace.access, Some(Access::Workspace));
        click(visual, "terminal-access");
        choose(visual, "New terminal · Full filesystem and network access");
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        wait(visual, |_| terminals(&fixture).len() == 2);
        let full = terminals(&fixture)
            .into_iter()
            .find(|info| info.id != workspace.id)
            .unwrap();
        assert_eq!(full.access, Some(Access::Unrestricted));
        wait(visual, |cx| {
            panel
                .read(cx)
                .mounted
                .as_ref()
                .and_then(|mounted| mounted.terminals.first())
                .is_some_and(|view| view.read(cx).info().is_some_and(|info| info.id == full.id))
        });
        let native = panel.read_with(visual, |panel, cx| {
            let native = panel
                .mounted
                .as_ref()
                .unwrap()
                .terminals
                .first()
                .unwrap()
                .clone();
            assert_eq!(native.read(cx).info().unwrap().id, full.id);
            native
        });
        visual.update(|window, cx| native.update(cx, |view, cx| view.focus(window, cx)));
        let current_revision = native.read_with(visual, |view, _| view.info().unwrap().revision);
        visual.simulate_input("printf '%s\\n' native-full-input");
        visual.simulate_keystrokes("enter");
        wait(visual, |_| {
            content(&fixture, full.id).contains("native-full-input")
        });
        let requests = fixture.transport.requests.lock().unwrap();
        let launches: Vec<_> = requests
            .iter()
            .filter_map(|request| match &request.command {
                Command::CreateTerminal(launch) => Some((request, launch)),
                _ => None,
            })
            .collect();
        assert_eq!(launches.len(), 2);
        assert_eq!(launches[0].1.access, Access::Workspace);
        assert_eq!(launches[1].1.access, Access::Unrestricted);
        for (request, launch) in launches {
            assert_eq!(request.target, fixture.node.id());
            assert_eq!(launch.worktree, fixture.session.worktree);
            let scope = request.plugin.as_ref().unwrap();
            assert!(scope.turn.is_none());
            assert_eq!(scope.session, Some(fixture.session.id));
        }
        let scope = requests
            .iter()
            .find_map(|request| match &request.command {
                Command::CreateTerminal(launch) if launch.access == Access::Unrestricted => {
                    request.plugin.clone()
                }
                _ => None,
            })
            .unwrap();
        let inputs: Vec<_> = requests
            .iter()
            .filter(|request| {
                matches!(&request.command,
            Command::InputTerminal { terminal, .. } if *terminal == full.id)
            })
            .collect();
        assert!(!inputs.is_empty());
        for request in inputs {
            assert_eq!(request.target, fixture.node.id());
            assert!(request.plugin.is_none());
            let Command::InputTerminal { revision, .. } = &request.command else {
                unreachable!()
            };
            assert_eq!(*revision, current_revision);
        }
        drop(requests);
        let mut scripted = fixture.binding.client.prepare(Command::InputTerminal {
            terminal: full.id,
            revision: current_revision,
            input: sailry_protocol::terminal::Input::Paste {
                text: "printf forbidden > scripted.txt\n".into(),
            },
        });
        scripted.plugin = Some(scope);
        let error = fixture
            .runtime
            .block_on(fixture.binding.client.execute(scripted))
            .unwrap_err();
        assert_eq!(error.code, sailry_protocol::ErrorCode::PermissionDenied);
        assert!(
            !fixture
                .directory
                .path()
                .join("project/scripted.txt")
                .exists()
        );
        visual.update(|window, _| window.remove_window());
        drop(native);
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn retries_original_full_cli_consent(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = fixture(remote);
        let programs = fixture.directory.path().join("project/programs");
        std::fs::create_dir(&programs).unwrap();
        let program = programs.join("claude");
        std::fs::write(&program,"#!/bin/sh\nprintf 'started\\n' >> starts.txt\nwhile IFS= read -r line; do printf '%s\\n' \"$line\"; done\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        fixture.execute(Command::SaveTerminalSettings(
            sailry_protocol::terminal::Settings {
                revision: 0,
                shell: String::new(),
                environment: [("PATH".into(), programs.to_str().unwrap().into())].into(),
            },
        ));
        let (panel, visual) = open(&fixture, cx);
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("claude:unrestricted")
        });
        fixture.transport.mode.store(12, Ordering::SeqCst);
        click(visual, "terminal-tools");
        choose(visual, "Claude · Full filesystem and network access");
        toast(visual, "Terminal result is unconfirmed");
        wait(visual, |_| terminals(&fixture).len() == 1);
        let info = terminals(&fixture).remove(0);
        assert_eq!(info.tool, Some(Tool::Claude));
        assert_eq!(info.access, Some(Access::Unrestricted));
        wait(visual, |_| {
            std::fs::read_to_string(fixture.directory.path().join("project/starts.txt"))
                .is_ok_and(|text| text == "started\n")
        });
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project/starts.txt")).unwrap(),
            "started\n"
        );
        click(visual, "terminal-new");
        assert_eq!(terminals(&fixture).len(), 1);
        click(visual, "terminal-retry");
        wait(visual, |cx| {
            !snapshot(&panel, cx).contains("terminal-retry")
        });
        let requests = fixture.transport.requests.lock().unwrap();
        let launches: Vec<_> = requests
            .iter()
            .filter(|request| matches!(request.command, Command::OpenToolTerminal { .. }))
            .collect();
        assert_eq!(launches.len(), 2);
        assert_eq!(launches[0], launches[1]);
        let Command::OpenToolTerminal { launch, .. } = &launches[0].command else {
            unreachable!()
        };
        assert_eq!(launch.access, Access::Unrestricted);
        assert!(launches[0].plugin.as_ref().unwrap().turn.is_none());
        assert_eq!(terminals(&fixture).len(), 1);
        assert_eq!(terminals(&fixture)[0].id, info.id);
        drop(requests);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}
