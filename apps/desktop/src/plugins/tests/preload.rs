use super::*;
use crate::settings::{Section, Workspace};
use sailry_link::{Admission, CancellationToken, Pending, Subscription, Transport};
use sailry_protocol::{Fault, NodeId, Output, Request, StreamId, Topic};
use std::sync::{Arc, Mutex};

pub(in crate::plugins::tests) struct Reads {
    inner: Arc<dyn Transport>,
    pause: Mutex<Option<(CancellationToken, CancellationToken)>>,
}

impl Reads {
    pub(in crate::plugins::tests) fn new(inner: Arc<dyn Transport>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            pause: Mutex::new(None),
        })
    }

    pub(in crate::plugins::tests) fn hold(&self) -> (CancellationToken, CancellationToken) {
        let entered = CancellationToken::new();
        let release = CancellationToken::new();
        *self.pause.lock().unwrap() = Some((entered.clone(), release.clone()));
        (entered, release)
    }
}

impl Transport for Reads {
    fn target(&self) -> NodeId {
        self.inner.target()
    }

    fn open(&self, stream: StreamId) -> Pending<'_, Result<sailry_link::Stream, Fault>> {
        self.inner.open(stream)
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            if matches!(
                request.command,
                Command::ReadPluginSettings { .. } | Command::ReadMediaSettings
            ) {
                let pause = self.pause.lock().unwrap().take();
                if let Some((entered, release)) = pause {
                    entered.cancel();
                    release.cancelled().await;
                }
            }
            self.inner.dispatch(request).await
        })
    }
}

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
fn settings_reuse_entities_until_context_changes(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let reads = Reads::new(fixture.transport.clone());
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
                owner.bind_providers(reads.clone(), fixture.runtime.clone(), "Node".into(), cx)
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
        let panel_id = panel.entity_id();
        let (root, script, path) = panel.read_with(visual, |panel, cx| {
            let mounted = panel.mounted.as_ref().unwrap();
            (
                mounted.root.entity_id(),
                mounted
                    .root
                    .read(cx)
                    .content()
                    .clone()
                    .downcast::<gpui_shell::ScriptView>()
                    .unwrap()
                    .entity_id(),
                mounted.cache_root().to_owned(),
            )
        });
        let bundle_reads = {
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
        assert_eq!(weak.upgrade().unwrap().entity_id(), panel_id);
        assert!(workspace.read_with(visual, |owner, _| owner.plugin_settings_panel().is_none()));
        assert!(path.exists());
        let (entered, release) = reads.hold();
        workspace.update(visual, |owner, cx| owner.open_plugin_settings("media", cx));
        visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            let panel = workspace.read(cx).plugin_settings_panel().unwrap();
            assert_eq!(panel.entity_id(), panel_id);
            let tree = snapshot(&panel, cx);
            assert!(tree.contains("media-settings"));
            assert!(!tree.contains("plugin-open-"));
        });
        assert!(visual.debug_bounds("plugin-panel-loading").is_none());
        wait(visual, |_| entered.is_cancelled());
        wait(visual, |cx| {
            workspace
                .read(cx)
                .plugin_settings_panel()
                .is_some_and(|panel| snapshot(&panel, cx).contains("media-settings"))
        });
        let panel = workspace.read_with(visual, |owner, _| owner.plugin_settings_panel().unwrap());
        assert_eq!(panel.entity_id(), panel_id);
        panel.read_with(visual, |panel, cx| {
            let mounted = panel.mounted.as_ref().unwrap();
            assert_eq!(mounted.root.entity_id(), root);
            assert_eq!(
                mounted
                    .root
                    .read(cx)
                    .content()
                    .clone()
                    .downcast::<gpui_shell::ScriptView>()
                    .unwrap()
                    .entity_id(),
                script,
            );
            assert_eq!(mounted.cache_root(), path);
        });
        assert_eq!(fixture.transport.views.lock().unwrap().len(), bundle_reads);
        release.cancel();
        let weak = panel.downgrade();
        drop(panel);
        host.update(visual, |host, cx| {
            host.visible = false;
            cx.notify();
        });
        workspace.update(visual, |owner, cx| owner.leave(cx));
        assert_eq!(weak.upgrade().unwrap().entity_id(), panel_id);
        assert!(path.exists());
        workspace.update(visual, |owner, cx| owner.open_plugin_settings("media", cx));
        host.update(visual, |host, cx| {
            host.visible = true;
            cx.notify();
        });
        wait(visual, |cx| {
            workspace
                .read(cx)
                .plugin_settings_panel()
                .is_some_and(|panel| {
                    panel.entity_id() == panel_id && snapshot(&panel, cx).contains("media-settings")
                })
        });
        assert_eq!(fixture.transport.views.lock().unwrap().len(), bundle_reads);
        workspace.update(visual, |owner, cx| {
            owner.bind_providers(
                fixture.controller.local(),
                fixture.runtime.clone(),
                "Other Node".into(),
                cx,
            )
        });
        wait(visual, |_| weak.upgrade().is_none() && !path.exists());
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

#[gpui::test]
fn settings_invalidation_releases_entities(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let root = fixture.directory.path().join("project/package");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("plugin.json")).unwrap()).unwrap();
        let extension = &mut manifest["extensions"]["dev.sailry.platform"];
        let entry = "dev.sailry.platform/desktop/settings.js";
        extension["settings_page"]["entry"] = entry.into();
        extension["settings_page"]["full_width"] = true.into();
        extension["desktop"]["resources"]
            .as_array_mut()
            .unwrap()
            .push(entry.into());
        std::fs::write(root.join("plugin.json"), manifest.to_string()).unwrap();
        let source = include_str!("navigation/entry.js");
        std::fs::write(root.join(entry), source).unwrap();
        let installed = fixture.install(0);
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let workspace = cx.new(|cx| Workspace::new(window, cx));
            workspace.update(cx, |workspace, cx| {
                workspace.bind_providers(
                    fixture.transport.clone(),
                    fixture.runtime.clone(),
                    "Node".into(),
                    cx,
                )
            });
            owner = Some(workspace.clone());
            Root::new(workspace, window, cx)
        });
        let workspace = owner.unwrap();
        wait(visual, |cx| {
            workspace
                .read(cx)
                .plugin_settings_entries(cx)
                .iter()
                .any(|entry| entry.name == installed.summary.name)
        });
        workspace.update(visual, |workspace, cx| {
            workspace.open_plugin_settings(&installed.summary.name, cx)
        });
        wait(visual, |cx| {
            workspace
                .read(cx)
                .plugin_settings_panel()
                .is_some_and(|panel| snapshot(&panel, cx).contains("entry-reads-1"))
        });
        let panel = workspace.read_with(visual, |workspace, _| {
            workspace.plugin_settings_panel().unwrap()
        });
        click(visual, "entry-edit");
        wait(visual, |cx| snapshot(&panel, cx).contains("entry-draft-1"));
        let id = panel.entity_id();
        let (root_view, script, path) = panel.read_with(visual, |panel, cx| {
            let mounted = panel.mounted.as_ref().unwrap();
            (
                mounted.root.downgrade(),
                mounted
                    .root
                    .read(cx)
                    .content()
                    .clone()
                    .downcast::<gpui_shell::ScriptView>()
                    .unwrap()
                    .downgrade(),
                mounted.cache_root().to_owned(),
            )
        });
        let previous = panel.downgrade();
        workspace.update(visual, |workspace, cx| {
            workspace.select(Section::General, cx)
        });
        drop(panel);
        let Output::Plugin(disabled) = fixture.execute(Command::SetPluginEnabled {
            name: installed.summary.name.clone(),
            expected_revision: installed.summary.revision,
            enabled: false,
        }) else {
            panic!("plugin expected");
        };
        wait(visual, |cx| {
            previous.upgrade().is_some_and(|panel| {
                panel
                    .read(cx)
                    .metadata
                    .read(cx)
                    .entries
                    .get(&installed.summary.name)
                    .is_some_and(|info| !info.summary.enabled)
            })
        });
        workspace.update(visual, |workspace, cx| {
            workspace.open_plugin_settings(&installed.summary.name, cx)
        });
        wait(visual, |cx| {
            workspace
                .read(cx)
                .plugin_settings_panel()
                .is_some_and(|panel| {
                    panel.entity_id() == id && snapshot(&panel, cx).contains("entry-draft-1")
                })
        });
        workspace.update(visual, |workspace, cx| {
            workspace.select(Section::General, cx)
        });
        std::fs::write(
            root.join(entry),
            source.replace("this.draft = 0;", "this.draft = 4;"),
        )
        .unwrap();
        let updated = fixture.install(disabled.summary.revision);
        assert_ne!(updated.summary.digest, installed.summary.digest);
        wait(visual, |_| {
            previous.upgrade().is_none()
                && root_view.upgrade().is_none()
                && script.upgrade().is_none()
                && !path.exists()
        });
        workspace.update(visual, |workspace, cx| {
            workspace.open_plugin_settings(&updated.summary.name, cx)
        });
        wait(visual, |cx| {
            workspace
                .read(cx)
                .plugin_settings_panel()
                .is_some_and(|panel| snapshot(&panel, cx).contains("entry-draft-4"))
        });
        let panel = workspace.read_with(visual, |workspace, _| {
            workspace.plugin_settings_panel().unwrap()
        });
        assert_ne!(panel.entity_id(), id);
        let previous = panel.downgrade();
        let root_view = panel.read_with(visual, |panel, _| {
            panel.mounted.as_ref().unwrap().root.downgrade()
        });
        workspace.update(visual, |workspace, cx| {
            workspace.select(Section::General, cx)
        });
        drop(panel);
        fixture.execute(Command::RemovePlugin {
            name: updated.summary.name.clone(),
            expected_revision: updated.summary.revision,
        });
        wait(visual, |_| {
            previous.upgrade().is_none() && root_view.upgrade().is_none()
        });
        wait(visual, |cx| {
            workspace
                .read(cx)
                .plugin_settings_entries(cx)
                .iter()
                .all(|entry| entry.name != updated.summary.name)
        });
        visual.update(|window, _| window.remove_window());
        drop(workspace);
        fixture.close();
    }
}
