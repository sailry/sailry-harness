use super::*;
use core::prelude::v1::test;

// The production Shell composes Kit's overlay layers around its pages.
struct Host(Entity<Panel>);
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

#[gpui::test]
fn progress_cancel_and_confirmation_reuse_kit(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::preferences::init(cx);
        crate::theme::init(cx);
    });
    let mut panel = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Panel::new(window, cx));
        panel = Some(view.clone());
        let host = cx.new(|_| Host(view));
        Root::new(host, window, cx)
    });
    let panel = panel.unwrap();
    visual.update(|window, cx| {
        let service = panel.read(cx).service.clone();
        service.update(cx, |service, cx| {
            service.state = State::Downloading(crate::updater::transfer::Progress::Downloading {
                copied: 35,
                total: Some(100),
            });
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
    let cancel = visual.debug_bounds("updates-cancel").unwrap();
    visual.simulate_click(cancel.center(), Modifiers::default());
    visual.update(|window, cx| {
        assert!(panel.read(cx).service.read(cx).cancelled());
        let service = panel.read(cx).service.clone();
        service.update(cx, |service, cx| {
            service.state = State::Ready("9.9.9".into());
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
    let install = visual.debug_bounds("updates-install").unwrap();
    visual.simulate_click(install.center(), Modifiers::default());
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    assert!(visual.debug_bounds("dialog-layer").is_some());
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    visual.update(|window, cx| {
        assert_eq!(
            panel.read(cx).service.read(cx).state,
            State::Ready("9.9.9".into())
        );
        window.draw(cx).clear(cx);
    });
    assert!(visual.debug_bounds("dialog-layer").is_none());
}

#[gpui::test]
fn preview_never_starts_update_work(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::preferences::init(cx);
        crate::theme::init(cx);
    });
    let mut panel = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Panel::new(window, cx));
        panel = Some(view.clone());
        let host = cx.new(|_| Host(view));
        Root::new(host, window, cx)
    });
    let panel = panel.unwrap();
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let check = visual.debug_bounds("updates-check").unwrap();
    visual.simulate_click(check.center(), Modifiers::default());
    visual.update(|_, cx| {
        assert_eq!(panel.read(cx).service.read(cx).state, State::Idle);
    });
}

#[cfg(target_os = "macos")]
#[gpui::test]
fn checks_and_downloads_an_authenticated_fixture_through_live_services(cx: &mut TestAppContext) {
    use crate::updater::tests::{fixture, transfer};
    use std::{collections::BTreeMap, sync::Arc};
    cx.executor().allow_parking();
    let staged = fixture::staged();
    let bytes = std::fs::read(&staged.archive).unwrap();
    let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    let directory = tempfile::tempdir().unwrap();
    let node = runtime
        .block_on(sailry_node_runtime::Node::start(
            directory.path().join("node"),
        ))
        .unwrap();
    let (base, serving) = runtime.block_on(transfer::server(|base| {
        let mut release = staged.selection.release.clone();
        release.url = format!("{base}/package.zip");
        BTreeMap::from([
            ("/update.json".into(), fixture::signed(&[release])),
            ("/package.zip".into(), bytes.clone()),
        ])
    }));
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        cx.set_global(crate::preferences::Preferences::open(
            node.profile().join("desktop/preferences.json"),
        ));
        cx.set_global(crate::backend::Services {
            runtime: runtime.clone(),
            link: node.link(),
            local: node.local(),
            relay_enabled: false,
        });
    });
    let mut panel = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Panel::new(window, cx));
        panel = Some(view.clone());
        let host = cx.new(|_| Host(view));
        Root::new(host, window, cx)
    });
    let panel = panel.unwrap();
    visual.update(|window, cx| {
        let service = panel.read(cx).service.clone();
        service.update(cx, |service, _| {
            let mut config = fixture::config();
            config.source = format!("{base}/update.json");
            service.fixture = Some(config);
        });
        window.draw(cx).clear(cx);
    });
    let check = visual.debug_bounds("updates-check").unwrap().center();
    visual.simulate_click(check, Modifiers::default());
    crate::conversation::live::tests::wait(visual, |cx| {
        matches!(panel.read(cx).service.read(cx).state, State::Available(_))
    });
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let download = visual.debug_bounds("updates-download").unwrap().center();
    visual.simulate_click(download, Modifiers::default());
    crate::conversation::live::tests::wait(visual, |cx| {
        matches!(panel.read(cx).service.read(cx).state, State::Ready(_))
    });
    assert_eq!(std::fs::read(&staged.archive).unwrap(), bytes);
    let client = sailry_client::Client::new(node.local());
    let sailry_protocol::Output::Snapshot(snapshot) = runtime
        .block_on(client.execute(client.prepare(sailry_protocol::Command::Snapshot)))
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    assert!(snapshot.sessions.is_empty());
    runtime.block_on(serving).unwrap();
    visual.update(|window, _| window.remove_window());
    runtime.block_on(node.shutdown()).unwrap();
}
