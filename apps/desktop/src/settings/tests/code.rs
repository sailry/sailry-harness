#![cfg(unix)]
use super::*;
use gpui_kit::component::{Root, WindowExt, dialog};
use sailry_protocol::{Command, Output, code};
mod fixture;
use fixture::{init, *};

#[gpui::test]
fn configures_captured_nodes(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.executable("rust-analyzer");
        let (owner, visual) = fixture.mount(cx);
        crate::settings::plugins::test_support::shown_settings(
            &owner,
            visual,
            "code-settings",
            true,
        );
        let panel = visual.update(|_, cx| selected(&owner, cx));
        tap(visual, "code-tabs-item-installed");
        wait(visual, |cx| ready(&owner, cx, "code-enabled-rust-analyzer"));
        tap(visual, "code-enabled-rust-analyzer");
        wait(visual, |_| {
            fixture
                .settings()
                .servers
                .get("rust-analyzer")
                .is_some_and(|value| value.disabled)
        });
        wait(visual, |cx| ready(&owner, cx, "code-edit-rust-analyzer"));
        tap(visual, "code-edit-rust-analyzer");
        input(visual, "code-command-0", "/fixture/custom-analyzer");
        input(visual, "code-command-1", "[\"--stdio\"]");
        input(visual, "code-command-2", "{\"check\":false}");
        input(visual, "code-command-3", "{\"FIXTURE_SETTING\":\"kept\"}");
        wait(visual, |_| {
            fixture.settings().servers["rust-analyzer"]
                .environment
                .as_ref()
                .is_some_and(|value| value.get("FIXTURE_SETTING").is_some())
        });
        tap(visual, "code-editor-close");
        let saved = fixture.settings();
        let custom = &saved.servers["rust-analyzer"];
        assert_eq!(custom.command.as_deref(), Some("/fixture/custom-analyzer"));
        assert_eq!(custom.args.as_deref(), Some(["--stdio".into()].as_slice()));
        assert_eq!(custom.config, Some(serde_json::json!({"check":false})));
        assert!(custom.disabled);
        for tree in [fixture.worktree, fixture.second] {
            fixture.bind(&owner, visual, false, Some(tree));
            assert_eq!(visual.update(|_, cx| selected(&owner, cx)), panel);
        }
        fixture.bind(&owner, visual, true, None);
        assert_ne!(visual.update(|_, cx| selected(&owner, cx)), panel);
        assert!(!panel.read_with(visual, |panel, _| panel.resource_active()));
        assert_eq!(fixture.settings(), saved);
        fixture.close(visual);
    }
}

#[gpui::test]
fn installs_and_removes_with_exact_consent(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let network = crate::plugins::fixture::code::Network::new();
        let fixture = Fixture::with_endpoint(remote, Some(network.endpoint.clone()));
        let (owner, visual) = fixture.mount(cx);
        let Output::CodeCatalog(catalog) = fixture.execute(Command::ReadCodeCatalog) else {
            panic!("catalog expected")
        };
        let consent = catalog
            .iter()
            .find(|language| language.id == "rust")
            .unwrap()
            .servers[0]
            .package
            .as_ref()
            .unwrap()
            .install
            .clone()
            .unwrap();
        tap(visual, "code-install-rust-analyzer");
        assert!(fixture.transport.requests.lock().unwrap().is_empty());
        wait(visual, |cx| ready(&owner, cx, "code-consent-submit"));
        tap(visual, "code-consent-submit");
        wait(visual, |cx| ready(&owner, cx, "code-edit-rust-analyzer"));
        let requests = fixture.transport.requests.lock().unwrap();
        assert!(requests.iter().any(|request| request.command
            == Command::InstallCodeServer {
                install: consent.clone()
            }));
        drop(requests);
        let Output::CodeInstallations(installed) = fixture.execute(Command::ListCodeInstallations)
        else {
            panic!("installations expected")
        };
        let receipt = installed[0].receipt.as_ref().unwrap();
        assert_eq!(receipt.version, network.asset.version);
        assert!(receipt.downloads.iter().all(|download| download.verified));
        tap(visual, "code-details-rust-analyzer");
        tap(visual, "code-details-uninstall");
        wait(visual, |cx| ready(&owner, cx, "code-consent-submit"));
        tap(visual, "code-consent-submit");
        wait(visual, |cx| {
            ready(&owner, cx, "code-update-rules")
                && matches!(fixture.execute(Command::ListCodeInstallations),Output::CodeInstallations(values) if values.iter().all(|value|value.receipt.is_none()))
        });
        fixture.close(visual);
    }
}

#[gpui::test]
fn cancels_download_from_subscription(cx: &mut TestAppContext) {
    use std::sync::atomic::Ordering;
    init(cx);
    for remote in [false, true] {
        let network = crate::plugins::fixture::code::Network::new();
        network.held.store(true, Ordering::SeqCst);
        let fixture = Fixture::with_endpoint(remote, Some(network.endpoint.clone()));
        let (owner, visual) = fixture.mount(cx);
        tap(visual, "code-install-rust-analyzer");
        wait(visual, |cx| ready(&owner, cx, "code-consent-submit"));
        tap(visual, "code-consent-submit");
        wait(visual, |cx| {
            ready(&owner, cx, "code-cancel-rust-analyzer")
                && network.hits.load(Ordering::SeqCst) > 0
        });
        tap(visual, "code-cancel-rust-analyzer");
        wait(visual, |cx| ready(&owner, cx, "code-update-rules"));
        let Output::CodeInstallations(values) = fixture.execute(Command::ListCodeInstallations)
        else {
            panic!("installations expected")
        };
        assert!(values.iter().all(|value| value.receipt.is_none()));
        assert!(
            values
                .iter()
                .any(|value| value.status == code::InstallStatus::Cancelled)
        );
        network.held.store(false, Ordering::SeqCst);
        fixture.close(visual);
    }
}
