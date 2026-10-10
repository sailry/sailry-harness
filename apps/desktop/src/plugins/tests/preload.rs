use super::*;
use crate::settings::{Section, Workspace};
use sailry_protocol::Output;

struct SettingsHost {
    workspace: Entity<Workspace>,
    visible: bool,
}
impl Render for SettingsHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .children(self.visible.then(|| self.workspace.clone()))
    }
}

#[gpui::test]
fn settings_reuse_files_without_retaining_views(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let Output::Plugin(media) = fixture.execute(Command::ReadPlugin {
            name: "media".into(),
        }) else {
            panic!("plugin expected");
        };
        *fixture.transport.view_target.lock().unwrap() = Some(media.summary.reference());
        let mut workspace = None;
        let mut host = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let owner = cx.new(|cx| Workspace::new(window, cx));
            owner.update(cx, |owner, cx| {
                owner.bind_providers(
                    fixture.transport.clone(),
                    fixture.runtime.clone(),
                    "Node".into(),
                    cx,
                )
            });
            workspace = Some(owner.clone());
            let container = cx.new(|_| SettingsHost {
                workspace: owner,
                visible: true,
            });
            host = Some(container.clone());
            Root::new(container, window, cx)
        });
        let workspace = workspace.unwrap();
        let host = host.unwrap();
        wait(visual, |cx| {
            workspace
                .read(cx)
                .plugin_settings_entries(cx)
                .iter()
                .any(|entry| entry.name == "media")
        });
        workspace.update(visual, |owner, cx| owner.preload_settings("media", cx));
        wait(visual, |_| {
            !fixture.transport.views.lock().unwrap().is_empty()
        });
        assert!(workspace.read_with(visual, |owner, _| owner.plugin_settings_panel().is_none()));
        workspace.update(visual, |owner, cx| owner.open_plugin_settings("media", cx));
        wait(visual, |cx| {
            workspace
                .read(cx)
                .plugin_settings_panel()
                .is_some_and(|panel| snapshot(&panel, cx).contains("media-settings"))
        });
        let panel = workspace.read_with(visual, |owner, _| owner.plugin_settings_panel().unwrap());
        let weak = panel.downgrade();
        let (root, path) = panel.read_with(visual, |panel, _| {
            let mounted = panel.mounted.as_ref().unwrap();
            (mounted.root.entity_id(), mounted.cache_root().to_owned())
        });
        let reads = {
            let requests = fixture.transport.views.lock().unwrap();
            assert!(!requests.is_empty());
            // Capacity retries reuse a request; a second resource load does not.
            assert!(
                requests.iter().all(|request| *request == requests[0]),
                "passive warming must not displace navigation: {requests:?}"
            );
            requests.len()
        };
        workspace.update(visual, |owner, cx| owner.select(Section::General, cx));
        drop(panel);
        wait(visual, |_| weak.upgrade().is_none());
        assert!(path.exists());
        workspace.update(visual, |owner, cx| owner.open_plugin_settings("media", cx));
        wait(visual, |cx| {
            workspace
                .read(cx)
                .plugin_settings_panel()
                .is_some_and(|panel| snapshot(&panel, cx).contains("media-settings"))
        });
        let panel = workspace.read_with(visual, |owner, _| owner.plugin_settings_panel().unwrap());
        panel.read_with(visual, |panel, _| {
            let mounted = panel.mounted.as_ref().unwrap();
            assert_ne!(mounted.root.entity_id(), root);
            assert_eq!(mounted.cache_root(), path);
        });
        assert_eq!(fixture.transport.views.lock().unwrap().len(), reads);
        let weak = panel.downgrade();
        drop(panel);
        host.update(visual, |host, cx| {
            host.visible = false;
            cx.notify();
        });
        workspace.update(visual, |owner, cx| owner.leave(cx));
        wait(visual, |_| weak.upgrade().is_none() && !path.exists());
        host.update(visual, |host, cx| {
            host.visible = true;
            cx.notify();
        });
        workspace.update(visual, |owner, cx| {
            owner.bind_providers(
                fixture.controller.local(),
                fixture.runtime.clone(),
                "Other Node".into(),
                cx,
            )
        });
        wait(visual, |_| weak.upgrade().is_none());
        wait(visual, |cx| {
            workspace
                .read(cx)
                .plugin_settings_panel()
                .is_some_and(|panel| {
                    panel.read(cx).binding.client.target() == fixture.controller.id()
                        && snapshot(&panel, cx).contains("media-settings")
                })
        });
        let panel = workspace.read_with(visual, |owner, _| owner.plugin_settings_panel().unwrap());
        panel.read_with(visual, |panel, _| {
            assert_ne!(panel.mounted.as_ref().unwrap().cache_root(), path)
        });
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(workspace);
        drop(host);
        fixture.close();
    }
}
