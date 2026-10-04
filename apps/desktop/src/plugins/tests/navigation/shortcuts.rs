use super::*;
use crate::preview::Page;
use crate::resources::shortcuts::OpenResource;
use sailry_protocol::{Output, plugin::desktop::ResourceKind};

fn key(resource: ResourceKind, cx: &App) -> Option<Keystroke> {
    cx.key_bindings()
        .borrow()
        .bindings_for_action(&OpenResource(resource))
        .next()
        .map(|binding| binding.keystrokes()[0].as_keystroke().clone())
}

#[gpui::test]
fn renderer_bindings_follow_the_captured_catalog(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        for name in ["browser", "commands"] {
            let Output::Plugin(info) = fixture.execute(Command::ReadPlugin { name: name.into() })
            else {
                panic!("bundled package expected")
            };
            fixture.execute(Command::SetPluginEnabled {
                name: name.into(),
                expected_revision: info.summary.revision,
                enabled: true,
            });
        }
        let (shell, visual) = super::super::files::mount(&fixture, remote, cx);
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.reveal_session(fixture.session.clone(), window, cx)
            })
        });
        wait(visual, |cx| {
            [
                ResourceKind::Documents,
                ResourceKind::Git,
                ResourceKind::Browser,
                ResourceKind::Terminal,
            ]
            .into_iter()
            .all(|resource| key(resource, cx).is_some())
        });
        visual.update(|_, cx| {
            assert_eq!(
                key(ResourceKind::Documents, cx),
                Some(Keystroke::parse("secondary-p").unwrap())
            );
            assert_eq!(
                key(ResourceKind::Git, cx),
                Some(Keystroke::parse("ctrl-shift-g").unwrap())
            );
            assert_eq!(
                key(ResourceKind::Browser, cx),
                Some(Keystroke::parse("secondary-t").unwrap())
            );
            assert_eq!(
                key(ResourceKind::Terminal, cx),
                Some(Keystroke::parse("ctrl-`").unwrap())
            );
        });
        visual.update(|window, cx| shell.update(cx, |shell, cx| shell.focus.focus(window, cx)));
        visual.simulate_keystrokes("secondary-p");
        wait(
            visual,
            |cx| matches!(&shell.read(cx).side_resource, Some(SideResource::Plugin(panel)) if panel.read(cx).document_scope(cx) == Some((fixture.node.id(), fixture.session.worktree))),
        );
        let read = || {
            let Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
                name: "files".into(),
            }) else {
                panic!("Files package expected")
            };
            info
        };
        let info = read();
        fixture.execute(Command::SetPluginEnabled {
            name: "files".into(),
            expected_revision: info.summary.revision,
            enabled: false,
        });
        wait(visual, |cx| key(ResourceKind::Documents, cx).is_none());
        assert!(visual.update(|_, cx| key(ResourceKind::Git, cx).is_some()));
        let info = read();
        fixture.execute(Command::SetPluginEnabled {
            name: "files".into(),
            expected_revision: info.summary.revision,
            enabled: true,
        });
        wait(visual, |cx| key(ResourceKind::Documents, cx).is_some());
        let info = read();
        fixture.execute(Command::RemovePlugin {
            name: "files".into(),
            expected_revision: info.summary.revision,
        });
        wait(visual, |cx| key(ResourceKind::Documents, cx).is_none());
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let before = shell
                    .side_resource
                    .as_ref()
                    .and_then(|resource| match resource {
                        SideResource::Plugin(panel) => Some(panel.clone()),
                        _ => None,
                    });
                shell.open_resource_shortcut(&OpenResource(ResourceKind::Documents), window, cx);
                let after = shell
                    .side_resource
                    .as_ref()
                    .and_then(|resource| match resource {
                        SideResource::Plugin(panel) => Some(panel.clone()),
                        _ => None,
                    });
                assert_eq!(before, after);
                shell.navigate(Page::Settings, window, cx);
            });
        });
        wait(visual, |cx| key(ResourceKind::Git, cx).is_none());
        visual.update(|window, _| window.remove_window());
        drop(shell);
        fixture.close();
    }
}
