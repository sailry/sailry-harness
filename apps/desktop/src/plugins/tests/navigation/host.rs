use super::*;
use crate::preview::Page;

const HEADER: &str = r#"
import { View, div } from 'gpui-kit';
import { Button } from 'gpui-component';
import { Header } from 'sailry';
import { Bounds } from 'sailry/test';

export default class Example extends View {
  init() {
    this.tabs = false;
  }

  render() {
    const content = this.tabs
      ? {tabs: {id: 'host-header-tabs', items: [{id: 'overview', label: 'Overview'}], selected: 'overview', close_label: 'Close'}}
      : {title: 'Host title'};
    return div().v_flex().size_full().gap_3()
      .child(Header.new('host-header', {content: JSON.stringify(content)}))
      .child(div().id(`header-state-${this.tabs}`))
      .child(new Button('header-tabs').relative().label('Tabs').child(Bounds.new('header-tabs')).on_click((_, cx) => {
        this.tabs = !this.tabs;
        cx.notify();
      }));
  }
}
"#;

#[track_caller]
fn header_state(panel: &WeakEntity<Panel>, visual: &mut VisualTestContext, tabs: bool) {
    let marker = format!("header-state-{tabs}");
    wait(visual, |cx| {
        let panel = panel.upgrade().unwrap();
        snapshot(&panel, cx).contains(&marker)
            && panel.read(cx).header().is_some_and(|header| {
                let header = header.read(cx);
                header.title(cx).is_some()
            })
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("activity-host-filter").is_none());
    assert_eq!(
        visual.debug_bounds("host-header-tab-overview").is_some(),
        tabs,
        "published tabs must render in the Shell header"
    );
}

#[gpui::test]
fn releases_navigation_when_selecting_another_host(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let path = fixture.directory.path().join("project/package/plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["extensions"]["dev.sailry.platform"]["desktop"]["navigation"] =
            serde_json::json!({"label":"Summary","icon":"reicon:files/file-text"});
        std::fs::write(path, manifest.to_string()).unwrap();
        let package = fixture.install(0);
        let (game, _server) = fixture.game_plugin_with("gomoku", &["ai_model"], |root| {
            let path = root.join("plugin.json");
            let mut manifest: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            manifest["extensions"]["dev.sailry.platform"]["scope"] = "host".into();
            std::fs::write(path, manifest.to_string()).unwrap();
            std::fs::write(root.join("dev.sailry.platform/desktop/main.js"), HEADER).unwrap();
        });
        cx.update(|cx| cx.set_global(fixture.services(remote)));
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| Shell::new(window, cx));
            owner = Some(shell.clone());
            Root::new(shell, window, cx)
        });
        let shell = owner.unwrap();
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
                shell.live.as_mut().unwrap().select(fixture.node.id(), cx);
            })
        });
        wait(visual, |cx| {
            shell.read(cx).extension_entries(cx).iter().any(|entry| {
                entry.node == fixture.node.id() && entry.package == package.summary.reference()
            })
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let entry = shell
                    .extension_entries(cx)
                    .into_iter()
                    .find(|entry| entry.package == package.summary.reference())
                    .unwrap();
                shell.open_extension(entry, window, cx);
            })
        });
        let panel = shell.read_with(visual, |shell, _| {
            shell
                .extensions
                .as_ref()
                .unwrap()
                .panel
                .as_ref()
                .unwrap()
                .downgrade()
        });
        wait(visual, |cx| {
            panel.upgrade().unwrap().read(cx).mounted.is_some()
        });
        wait(visual, |cx| {
            shell
                .read(cx)
                .extension_entries(cx)
                .iter()
                .any(|entry| entry.package == game.summary.reference())
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let entry = shell
                    .extension_entries(cx)
                    .into_iter()
                    .find(|entry| entry.package == game.summary.reference())
                    .unwrap();
                shell.open_extension(entry, window, cx);
            })
        });
        let game_panel = shell.read_with(visual, |shell, _| {
            shell
                .extensions
                .as_ref()
                .unwrap()
                .panel
                .as_ref()
                .unwrap()
                .downgrade()
        });
        header_state(&game_panel, visual, false);
        click(visual, "header-tabs");
        header_state(&game_panel, visual, true);
        click(visual, "header-tabs");
        header_state(&game_panel, visual, false);
        assert!(
            panel.upgrade().is_some(),
            "switching entries must retain the hidden panel"
        );
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .live
                    .as_mut()
                    .unwrap()
                    .select(fixture.controller.id(), cx);
            })
        });
        wait(visual, |cx| {
            let shell = shell.read(cx);
            shell.live.as_ref().unwrap().view.connected
                && shell.extensions.as_ref().unwrap().panel.is_none()
                && panel.upgrade().is_none()
                && game_panel.upgrade().is_none()
        });
        assert_eq!(shell.read_with(visual, |shell, _| shell.page), Page::Host);
        visual.update(|window, _| window.remove_window());
        drop(shell);
        fixture.close();
    }
}
