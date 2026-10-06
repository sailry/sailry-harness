use super::*;
use crate::ssh_fixture as server;
use core::prelude::v1::test;
use std::time::Instant;

struct Frame(Entity<Installer>);
impl Render for Frame {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(self.0.clone())
            .child(self.0.update(cx, |view, cx| view.footer(cx)))
    }
}
#[gpui::test]
fn credentials_and_installation_trust(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    rust_i18n::set_locale("zh-CN");
    let directory = tempfile::tempdir().unwrap();
    let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    let node = runtime
        .block_on(sailry_node_runtime::Node::start(
            directory.path().join("node"),
        ))
        .unwrap();
    let server = runtime.block_on(server::Server::start(directory.path().into(), 63));
    let services = Services {
        runtime: runtime.clone(),
        link: node.link(),
        local: node.local(),
        relay_enabled: false,
    };
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut installer = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let entity = cx.new(|cx| Installer::new(services, window, cx));
        installer = Some(entity.clone());
        Root::new(cx.new(|_| Frame(entity)), window, cx)
    });
    let installer = installer.unwrap();
    visual.update(|window, cx| {
        installer.read(cx).editor.clone().update(cx, |editor, cx| {
            for (index, value) in [
                "Development host".to_string(),
                "127.0.0.1".into(),
                server.port.to_string(),
                "fixture".into(),
            ]
            .into_iter()
            .enumerate()
            {
                editor.fields[index].update(cx, |input, cx| input.set_value(value, window, cx));
            }
            editor.secret.update(cx, |input, cx| {
                input.set_value("isolated-ssh-password", window, cx)
            });
        });
        window.draw(cx).clear(cx);
    });
    let save = visual.debug_bounds("ssh-save").unwrap();
    visual.simulate_click(save.center(), Modifiers::default());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.run_until_parked();
        if installer.read_with(visual, |view, _| view.key.is_some()) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        server
            .authentication
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    assert!(server.commands.lock().unwrap().is_empty());
    installer.read_with(visual, |view, _| {
        assert!(!view.form);
        assert!(view.profile.is_some());
        assert!(view.error.is_none());
    });
    visual.update(|window, cx| {
        installer.update(cx, |view, cx| {
            view.key = None;
            view.progress = InstallProgress::Installing;
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
    assert!(visual.debug_bounds("host-install-progress").is_some());
    for (code, key) in [
        (ErrorCode::Unavailable, "host_install_failed"),
        (ErrorCode::OutcomeUnknown, "host_install_unknown"),
    ] {
        visual.update(|window, cx| {
            window.clear_notifications(cx);
            installer.update(cx, |view, cx| {
                view.complete(Err(Fault::new(code, "Deployment fixture")), window, cx);
            });
        });
        crate::feedback::tests::shown(visual);
        crate::feedback::tests::settle(visual);
        assert_eq!(visual.update(crate::feedback::tests::summary), tr(key));
        assert!(visual.debug_bounds("error-toast-detail").is_some());
        assert!(visual.debug_bounds("host-install-error").is_none());
        installer.read_with(visual, |view, cx| {
            assert!(view.profile.is_some());
            assert!(view.form);
            assert!(view.key.is_none());
            assert!(!view.editor.read(cx).locked);
        });
        assert!(visual.debug_bounds("ssh-save").is_some());
        assert!(visual.debug_bounds("ssh-field-1").is_some());
        assert!(visual.debug_bounds("host-install-progress").is_none());
    }
    visual.update(|_, cx| installer.update(cx, |view, _| view.cancel()));
    runtime.block_on(server.close());
    runtime.block_on(node.shutdown()).unwrap();
}

#[gpui::test]
fn connection_failure_preserves_form_and_retry(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    let node = runtime
        .block_on(sailry_node_runtime::Node::start(
            directory.path().join("node"),
        ))
        .unwrap();
    let server = runtime.block_on(server::Server::start(directory.path().into(), 64));
    let services = Services {
        runtime: runtime.clone(),
        link: node.link(),
        local: node.local(),
        relay_enabled: false,
    };
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut installer = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let entity = cx.new(|cx| Installer::new(services, window, cx));
        installer = Some(entity.clone());
        Root::new(cx.new(|_| Frame(entity)), window, cx)
    });
    let installer = installer.unwrap();
    visual.update(|window, cx| {
        installer.read(cx).editor.clone().update(cx, |editor, cx| {
            for (field, value) in editor.fields.iter().zip([
                "Test host".to_string(),
                "127.0.0.1".into(),
                server.port.to_string(),
                "fixture".into(),
            ]) {
                field.update(cx, |input, cx| input.set_value(value, window, cx));
            }
            editor.secret.update(cx, |input, cx| {
                input.set_value("incorrect-fixture-password", window, cx)
            });
        });
        window.draw(cx).clear(cx);
    });
    let save = visual.debug_bounds("ssh-save").unwrap().center();
    visual.simulate_click(save, Modifiers::default());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.run_until_parked();
        if installer.read_with(visual, |view, _| view.key.is_some()) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let id = installer.read_with(visual, |view, _| view.profile.as_ref().unwrap().id);
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let trust = visual.debug_bounds("host-install-trust").unwrap().center();
    visual.simulate_click(trust, Modifiers::default());
    for attempt in 0..2 {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            visual.run_until_parked();
            if installer.read_with(visual, |view, _| view.error.is_some()) {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        crate::feedback::tests::shown(visual);
        crate::feedback::tests::settle(visual);
        assert_eq!(
            visual.update(crate::feedback::tests::summary),
            tr("host_install_failed")
        );
        installer.read_with(visual, |view, cx| {
            assert_eq!(
                view.error.as_ref().unwrap().code,
                ErrorCode::PermissionDenied
            );
            assert!(view.form);
            assert!(view.request.is_none());
            assert!(view.key.is_none());
            let profile = view.profile.as_ref().unwrap();
            assert_eq!(profile.id, id);
            let editor = view.editor.read(cx);
            assert!(!editor.locked);
            assert_eq!(editor.fields[1].read(cx).value(), "127.0.0.1");
            assert_eq!(editor.secret.read(cx).value(), "incorrect-fixture-password");
        });
        assert!(server.commands.lock().unwrap().is_empty());
        assert!(visual.debug_bounds("host-install-progress").is_none());
        assert!(visual.debug_bounds("host-install-error").is_none());
        assert!(visual.debug_bounds("ssh-save").is_some());
        if attempt == 0 {
            let save = visual.debug_bounds("ssh-save").unwrap().center();
            visual.simulate_click(save, Modifiers::default());
        }
    }
    visual.update(|_, cx| installer.update(cx, |view, _| view.cancel()));
    runtime.block_on(server.close());
    runtime.block_on(node.shutdown()).unwrap();
}

#[gpui::test]
fn sidebar_host_filter(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    let node = runtime
        .block_on(sailry_node_runtime::Node::start(
            directory.path().join("node"),
        ))
        .unwrap();
    let controller = runtime
        .block_on(sailry_link::Link::controller(
            directory.path().join("mobile"),
            Default::default(),
        ))
        .unwrap();
    let invitation = controller.handle().invite().unwrap();
    runtime
        .block_on(node.link().pair(invitation.ticket()))
        .unwrap();
    let key: String = controller
        .handle()
        .address()
        .id
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(Services {
            runtime: runtime.clone(),
            link: node.link(),
            local: node.local(),
            relay_enabled: false,
        });
    });
    let mut shell = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let entity = cx.new(|cx| Shell::new(window, cx));
        shell = Some(entity.clone());
        Root::new(entity, window, cx)
    });
    let shell = shell.unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.run_until_parked();
        if visual.update(|_, cx| {
            crate::preferences::data(cx)
                .devices
                .as_ref()
                .and_then(|devices| devices.get(&key))
                .is_some_and(|device| !device.execution)
        }) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.live.as_ref().unwrap().hosts.len()),
        1
    );
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let add = visual.debug_bounds("host-add").unwrap();
    visual.simulate_click(add.center(), Modifiers::default());
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("ssh-save").is_some());
    visual.update(|window, cx| window.close_dialog(cx));
    runtime.block_on(controller.close()).unwrap();
    runtime.block_on(node.shutdown()).unwrap();
}
