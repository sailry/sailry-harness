use super::*;
use core::prelude::v1::test;
use sailry_node_runtime::Node;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[gpui::test]
#[ignore = "requires isolated workerd from pnpm test:desktop"]
fn sharing_lifecycle(cx: &mut TestAppContext) {
    // This integration test intentionally receives real Tokio/network wakeups.
    cx.executor().allow_parking();
    let endpoint = std::env::var("SAILRY_TEST_RELAY").expect("test relay address");
    let directory = tempfile::tempdir().unwrap();
    let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    let host = runtime
        .block_on(Node::start(directory.path().join("host")))
        .unwrap();
    let client = runtime
        .block_on(Node::start(directory.path().join("client")))
        .unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        cx.set_global(Services {
            runtime: runtime.clone(),
            link: host.link(),
            local: host.local(),
            relay_enabled: false,
        });
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Connections::new(window, cx));
        view.update(cx, |view, _| {
            view.endpoint = endpoint.clone();
        });
        entity = Some(view.clone());
        Root::new(cx.new(|_| Frame(view)), window, cx)
    });
    let view = entity.unwrap();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let button = visual.debug_bounds("pairing-share").unwrap();
    visual.simulate_click(button.center(), Modifiers::default());
    let code = wait_code(visual, &view);
    let relay = Relay::new(&endpoint).unwrap();
    runtime
        .block_on(relay.pair(&client.link(), &code, &RequestId::default()))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        visual.run_until_parked();
        if view.read_with(visual, |view, _| view.status == tr("pairing_success")) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "pairing UI did not receive completion"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let button = visual.debug_bounds("pairing-share").unwrap();
    visual.simulate_click(button.center(), Modifiers::default());
    let cancelled = wait_code(visual, &view);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    std::thread::sleep(Duration::from_millis(20));
    assert!(
        runtime
            .block_on(relay.pair(&client.link(), &cancelled, &RequestId::default()))
            .is_err()
    );
    assert!(view.read_with(visual, |view, _| view.code.is_none() && view.stop.is_none()));
    let target = runtime
        .block_on(Node::start(directory.path().join("target")))
        .unwrap();
    let mut invitation = target.link().invite().unwrap();
    let code = runtime
        .block_on(relay.publish(&invitation, &RequestId::default()))
        .unwrap();
    let button = visual.debug_bounds("pairing-add").unwrap();
    visual.simulate_click(button.center(), Modifiers::default());
    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.pin
                .update(cx, |input, cx| input.set_value(code.code, window, cx))
        });
        let _ = window.draw(cx);
    });
    let button = visual.debug_bounds("pairing-connect").unwrap();
    visual.simulate_click(button.center(), Modifiers::default());
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        visual.run_until_parked();
        if view.read_with(visual, |view, _| view.status == tr("pairing_success")) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "pairing UI did not redeem the code"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    runtime.block_on(invitation.paired()).unwrap();
    let remote = sailry_client::Client::new(host.link().remote(target.link().address()));
    assert!(
        runtime
            .block_on(remote.execute(remote.prepare(sailry_protocol::Command::Snapshot)))
            .is_ok()
    );
    runtime.block_on(target.shutdown()).unwrap();
    runtime.block_on(host.shutdown()).unwrap();
    runtime.block_on(client.shutdown()).unwrap();
}

fn wait_code(cx: &VisualTestContext, view: &Entity<Connections>) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        cx.run_until_parked();
        if let Some(code) = view.read_with(cx, |view, _| view.code.clone()) {
            return code;
        }
        assert!(
            Instant::now() < deadline,
            "pairing UI did not receive a code"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct Frame(Entity<Connections>);
impl Render for Frame {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}
