use super::*;

#[gpui::test]
fn retries_reads(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let package = fixture.install(0);
        fixture.configure(&package, false);
        fixture.transport.mode.store(4, Ordering::SeqCst);
        let (panel, visual) = mount(&fixture, cx);
        open_package(visual, &panel, "project-summary");
        toast(visual, "Could not read settings");
        assert!(panel.read_with(visual, |_, cx| {
            !snapshot(&panel, cx).contains("summary-status-settingsFailed")
        }));
        control(visual, 130., 64.);
        assert!(
            !fixture
                .directory
                .path()
                .join("project/project-summary.md")
                .exists()
        );
        control(visual, 40., 64.);
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("summary-status-ready")
        });
        assert!(
            !panel
                .read_with(visual, |_, cx| snapshot(&panel, cx))
                .contains("notes.txt")
        );
        assert_eq!(
            fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| matches!(request.command, Command::ReadPluginSettings { .. }))
                .count(),
            2
        );
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn reloads_after_settings_change(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let package = fixture.install(0);
        let (panel, visual) = mount(&fixture, cx);
        open_package(visual, &panel, "project-summary");
        wait(visual, |cx| snapshot(&panel, cx).contains("notes.txt"));
        let previous = panel.read_with(visual, |panel, cx| {
            panel
                .mounted
                .as_ref()
                .unwrap()
                .root
                .read(cx)
                .content()
                .clone()
                .downcast::<gpui_shell::ScriptView>()
                .unwrap()
                .downgrade()
        });
        fixture.transport.mode.store(2, Ordering::SeqCst);
        let updated = fixture.configure(&package, false);
        assert_eq!(updated.summary.digest, package.summary.digest);
        assert_ne!(
            updated.summary.settings_revision,
            package.summary.settings_revision
        );
        wait(visual, |_| fixture.transport.entered.is_cancelled());
        assert!(
            visual.update(|_, cx| {
                let state = panel.read(cx);
                state.loading
                    && state.mounted.is_some()
                    && state
                        .metadata
                        .read(cx)
                        .entries
                        .contains_key(&package.summary.name)
                    && snapshot(&panel, cx).contains("notes.txt")
            }),
            "settings refresh must keep the previous presentation until replacement"
        );
        fixture.transport.release.cancel();
        wait(visual, |cx| {
            panel.read(cx).selected == Some(updated.summary.reference())
                && !panel.read(cx).loading
                && snapshot(&panel, cx).contains("summary-status-ready")
        });
        wait(visual, |_| {
            previous.upgrade().is_none() && fixture.transport.files.load(Ordering::SeqCst) == 1
        });
        let stale = fixture.binding.client.prepare(Command::ReadPluginView {
            surface: sailry_protocol::plugin::desktop::Surface::Workspace,
            package: package.summary.reference(),
        });
        assert_eq!(
            fixture
                .runtime
                .block_on(fixture.binding.client.execute(stale))
                .unwrap_err()
                .code,
            sailry_protocol::ErrorCode::RevisionConflict
        );
        assert!(
            !panel
                .read_with(visual, |_, cx| snapshot(&panel, cx))
                .contains("notes.txt")
        );
        control(visual, 24., 24.);
        wait(visual, |cx| snapshot(&panel, cx).contains("notes.txt"));
        control(visual, 40., 64.);
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("summary-status-ready")
        });
        assert!(
            panel
                .read_with(visual, |_, cx| snapshot(&panel, cx))
                .contains("notes.txt")
        );
        let requests = fixture.transport.requests.lock().unwrap();
        let reads: Vec<_> = requests
            .iter()
            .filter(|request| matches!(request.command, Command::ReadPluginSettings { .. }))
            .collect();
        assert_eq!(reads.len(), 2);
        assert_eq!(
            reads[1].plugin.as_ref().unwrap().package,
            updated.summary.reference()
        );
        assert!(
            requests
                .iter()
                .all(|request| !matches!(request.command, Command::SavePluginSettings { .. }))
        );
        drop(requests);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn edits_disabled_package(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let root = fixture.directory.path().join("project/package");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("plugin.json")).unwrap()).unwrap();
        let extension = &mut manifest["extensions"]["dev.sailry.platform"];
        extension["settings_page"]["entry"] = "dev.sailry.platform/desktop/settings.js".into();
        extension["desktop"]["resources"]
            .as_array_mut()
            .unwrap()
            .push("dev.sailry.platform/desktop/settings.js".into());
        std::fs::write(root.join("plugin.json"), manifest.to_string()).unwrap();
        std::fs::write(root.join("dev.sailry.platform/desktop/settings.js"), r#"
import { View, div } from "gpui-kit";
import { Button } from "gpui-component";
import { context, prepare, execute, forget } from "sailry";
export default class Settings extends View {
  init(_props, cx) {
    this.scope = JSON.parse(context()); this.value = null;
    cx.spawn(async cx => {
      const id = prepare(JSON.stringify({kind:"read_plugin_settings",data:{package:this.scope.package}}));
      const result = JSON.parse(await execute(id)); forget(id);
      this.value = result.Ok.data.values.include_untracked; cx.notify();
    });
  }
  render(cx) {
    return div().v_flex().p_4().child(div().id(`settings-value-${this.value}`).child(String(this.value)))
      .child(new Button("settings-toggle").label("Toggle").on_click((_event,cx) => {
        cx.spawn(async cx => {
          const id = prepare(JSON.stringify({kind:"save_plugin_settings",data:{package:this.scope.package, values:{include_untracked:!this.value}, secrets:{}}}));
          const result = JSON.parse(await execute(id));
          if (result.Ok?.kind === 'plugin_settings') {
            this.scope.package = result.Ok.data.package;
            this.value = result.Ok.data.values.include_untracked;
          }
          forget(id); cx.notify();
        });
      }));
  }
}
"#).unwrap();
        let installed = fixture.install(0);
        fixture.execute(Command::SetPluginEnabled {
            name: installed.summary.name.clone(),
            expected_revision: installed.summary.revision,
            enabled: false,
        });
        let mut panel = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| Panel::settings(fixture.binding.clone(), cx));
            panel = Some(view.clone());
            Root::new(view, window, cx)
        });
        let panel = panel.unwrap();
        wait(visual, |cx| {
            panel.read(cx).connected && panel.read(cx).metadata.read(cx).settled()
        });
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.open(installed.summary.reference(), window, cx)
            })
        });
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("settings-value-true")
        });
        let original = panel.read_with(visual, |panel, cx| {
            panel
                .mounted
                .as_ref()
                .unwrap()
                .root
                .read(cx)
                .content()
                .clone()
                .downcast::<gpui_shell::ScriptView>()
                .unwrap()
        });
        control(visual, 45., 55.);
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("settings-value-false")
        });
        assert_eq!(
            original,
            panel.read_with(visual, |panel, cx| {
                panel
                    .mounted
                    .as_ref()
                    .unwrap()
                    .root
                    .read(cx)
                    .content()
                    .clone()
                    .downcast::<gpui_shell::ScriptView>()
                    .unwrap()
            })
        );
        let requests = fixture.transport.requests.lock().unwrap();
        assert!(requests.iter().any(|request| matches!(
            request.command,
            Command::SavePluginSettings { .. }
        ) && request.plugin.as_ref().unwrap().surface
            == sailry_protocol::plugin::desktop::Surface::Settings));
        drop(requests);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}
