use super::*;
use crate::{resources::SideResource, shell::Shell};
mod catalog;
mod desktop;
mod host;
mod native;
mod shortcuts;
mod switching;
mod workspace;

fn declared_navigation(
    fixture: &Fixture,
    client: &sailry_client::Client,
    scope: sailry_protocol::plugin::Scope,
) -> std::collections::BTreeSet<String> {
    let execute = |command| {
        fixture
            .runtime
            .block_on(client.execute(client.prepare(command)))
            .unwrap()
    };
    let sailry_protocol::Output::Plugins(packages) = execute(Command::ListPlugins) else {
        panic!("plugin inventory expected");
    };
    packages
        .into_iter()
        .filter(|package| package.enabled)
        .filter_map(|package| {
            let sailry_protocol::Output::Plugin(info) = execute(Command::ReadPlugin {
                name: package.name.clone(),
            }) else {
                panic!("plugin metadata expected");
            };
            assert!(
                info.issues.is_empty(),
                "{}: {:?}",
                package.name,
                info.issues
            );
            info.extension
                .filter(|extension| extension.scope == scope)
                .and_then(|extension| extension.desktop)
                .and_then(|desktop| desktop.navigation)
                .map(|_| package.name)
        })
        .collect()
}

fn entry_names(shell: &Shell, cx: &App) -> std::collections::BTreeSet<String> {
    shell
        .extension_entries(cx)
        .into_iter()
        .map(|entry| entry.package.name)
        .collect()
}

#[gpui::test]
fn preserves_session_binding_until_closed(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        fixture.install(0);
        cx.update(|cx| cx.set_global(fixture.services(remote)));
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| Shell::new(window, cx));
            owner = Some(shell.clone());
            Root::new(shell, window, cx)
        });
        let shell = owner.unwrap();
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .hosts
                .contains_key(&fixture.node.id())
        });
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(fixture.node.id(), cx)
            })
        });
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .sessions
                        .iter()
                        .any(|session| session.id == fixture.session.id)
                })
        });
        let session = Box::leak(format!("live-session-{}", fixture.session.id).into_boxed_str());
        if visual.debug_bounds(session).is_none() {
            click(
                visual,
                Box::leak(
                    format!("live-project-{}", fixture.session.project.unwrap()).into_boxed_str(),
                ),
            );
        }
        click(visual, session);
        let source = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
        click(visual, "toggle-details");
        wait(
            visual,
            |cx| matches!(&shell.read(cx).side_resource, Some(SideResource::Launcher(Some(panel))) if !panel.read(cx).launcher_entries(cx).is_empty()),
        );
        click(visual, "launch-extension-0");
        let panel = shell.read_with(visual, |shell, _| {
            match shell.side_resource.as_ref().unwrap() {
                SideResource::Plugin(panel) => panel.clone(),
                _ => panic!("plugin panel expected"),
            }
        });
        wait(visual, |cx| panel.read(cx).metadata.read(cx).settled());
        wait(visual, |cx| snapshot(&panel, cx).contains("notes.txt"));
        assert!(visual.debug_bounds("plugin-back").is_none());
        assert_eq!(
            panel.read_with(visual, |panel, cx| panel.launcher_entries(cx)[0].0.clone()),
            if &*rust_i18n::locale() == "zh-CN" {
                "项目摘要"
            } else {
                "Project summary"
            }
        );
        let released = panel.downgrade();
        drop(panel);
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .live
                    .as_mut()
                    .unwrap()
                    .select(fixture.controller.id(), cx)
            })
        });
        wait(visual, |cx| {
            shell.read(cx).live.as_ref().unwrap().view.connected
        });
        // Host selection does not retarget the still-open session's inspector.
        assert!(shell.read_with(visual, |shell, cx| matches!(&shell.side_resource, Some(SideResource::Plugin(panel)) if panel.read(cx).binding.client.target() == fixture.node.id())));
        visual.update(|_, cx| shell.update(cx, |shell, cx| shell.close_resource_panel(cx)));
        wait(visual, |cx| {
            shell.read(cx).side_resource.is_none() && released.upgrade().is_none()
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.navigate(crate::preview::Page::Host, window, cx)
            })
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.open_plugin_panel(source.clone(), window, cx)
            })
        });
        assert!(shell.read_with(visual, |shell, _| shell.side_resource.is_none()));
        visual.update(|window, _| window.remove_window());
        drop(shell);
        drop(source);
        fixture.close();
    }
}
