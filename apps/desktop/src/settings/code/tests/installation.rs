use super::*;
use gpui_kit::test::TestWindowExt as _;
use sailry_protocol::{Request, code::InstallStatus};

use super::network::Network;

fn request(fixture: &Fixture) -> Request {
    fixture
        .transport
        .requests
        .lock()
        .unwrap()
        .iter()
        .find(|request| matches!(request.command, Command::InstallCodeServer { .. }))
        .unwrap()
        .clone()
}

#[gpui::test]
fn uses_captured_consent_through_lifecycle(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let network = Network::new();
        let fixture = Fixture::with_endpoint(remote, Some(network.endpoint.clone()));
        let (owner, cx) = fixture.mount(cx);
        let panel = cx.update(|_, cx| selected(&owner, cx));
        let consent = cx.update(|_, cx| {
            panel.read(cx).workspace.as_ref().unwrap().languages[0].servers[0]
                .package
                .as_ref()
                .unwrap()
                .install
                .clone()
                .unwrap()
        });
        assert_eq!(consent.version, network.asset.version);
        assert_eq!(consent.urls, vec![network.asset.asset_url.clone()]);
        assert!(!consent.licenses.is_empty());
        tap(cx, "code-install-rust-analyzer");
        assert!(fixture.transport.requests.lock().unwrap().is_empty());
        assert!(cx.debug_bounds("code-consent-details").is_some());
        for selector in [
            "code-consent-code_package",
            "code-consent-code_version",
            "code-consent-code_downloads",
            "code-consent-code_licenses",
            "code-consent-code_install_command",
        ] {
            assert!(cx.debug_bounds(selector).is_some(), "missing {selector}");
        }
        tap(cx, "code-consent-submit");
        wait(cx, |cx| {
            !panel.read(cx).pending
                && !panel.read(cx).loading
                && panel
                    .read(cx)
                    .installations
                    .iter()
                    .any(|installation| installation.status == InstallStatus::Ready)
        });
        let request = request(&fixture);
        assert_eq!(
            request.command,
            Command::InstallCodeServer { install: consent }
        );
        let installation = cx.update(|_, cx| panel.read(cx).installations[0].clone());
        let receipt = installation.receipt.unwrap();
        assert_eq!(receipt.version, network.asset.version);
        assert!(receipt.downloads.iter().all(|download| download.verified));
        assert_eq!(network.hits.load(Ordering::SeqCst), 1);
        assert!(cx.debug_bounds("code-tab-installed").is_some());
        assert!(cx.debug_bounds("code-edit-rust-analyzer").is_some());
        assert!(cx.debug_bounds("code-install-rust-analyzer").is_none());
        tap(cx, "code-details-rust-analyzer");
        assert!(cx.debug_bounds("code-details-update").is_some());
        assert!(cx.debug_bounds("code-details-uninstall").is_some());
        assert!(cx.debug_bounds("code-detail-version").is_some());
        tap(cx, "code-details-close");
        fixture.transport.drop_next.store(true, Ordering::SeqCst);
        tap(cx, "code-start-rust-analyzer");
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
        let startup = cx.update(|_, cx| {
            panel.read(cx).workspace.as_ref().unwrap().languages[0].servers[0]
                .startup
                .clone()
                .unwrap()
        });
        tap(cx, "code-consent-submit");
        wait(cx, |cx| {
            !panel.read(cx).pending
                && panel.read(cx).request.is_some()
                && panel
                    .read(cx)
                    .servers
                    .iter()
                    .any(|server| server.status == code::Status::Ready)
        });
        let original = cx.update(|_, cx| panel.read(cx).request.clone().unwrap());
        tap(cx, "code-retry");
        wait(cx, |cx| {
            !panel.read(cx).pending && !panel.read(cx).loading && panel.read(cx).request.is_none()
        });
        let requests = fixture.transport.requests.lock().unwrap();
        let repeats: Vec<_> = requests
            .iter()
            .filter(|request| request.id == original.id)
            .collect();
        assert_eq!(repeats.len(), 2);
        assert_eq!(repeats[0], repeats[1]);
        drop(requests);
        assert!(
            fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| request.command
                    == Command::StartCodeServer {
                        startup: startup.clone()
                    })
        );
        let Output::CodeServers(servers) = fixture.execute(Command::ListCodeServers {
            worktree: fixture.worktree,
        }) else {
            panic!("servers expected")
        };
        assert_eq!(servers[0].startup, startup);
        assert_eq!(servers[0].status, code::Status::Ready);
        let log = fixture.root.join("messages");
        let messages = std::fs::read_to_string(log).unwrap();
        let starts = messages
            .lines()
            .filter(|line| {
                serde_json::from_str::<serde_json::Value>(line).unwrap()["method"] == "initialize"
            })
            .count();
        assert_eq!(starts, 1);
        assert!(cx.debug_bounds("code-stop-rust-analyzer").is_some());
        assert!(cx.debug_bounds("code-start-rust-analyzer").is_none());
        cx.update(|window, _| {
            assert_eq!(
                window.find("code-stop-rust-analyzer").label(),
                Some(crate::tr("code_pause").as_ref())
            );
        });
        let settings = fixture.settings();
        fixture.transport.drop_next.store(true, Ordering::SeqCst);
        tap(cx, "code-stop-rust-analyzer");
        wait(cx, |cx| {
            !panel.read(cx).pending && panel.read(cx).request.is_some()
        });
        let stopped = cx.update(|_, cx| panel.read(cx).request.clone().unwrap());
        assert_eq!(stopped.target, fixture.node.id());
        assert_eq!(
            stopped.command,
            Command::StopCodeServer {
                startup: startup.clone()
            }
        );
        tap(cx, "code-retry");
        wait(cx, |cx| {
            !panel.read(cx).pending && !panel.read(cx).loading && panel.read(cx).request.is_none()
        });
        assert_eq!(fixture.settings(), settings);
        assert!(cx.debug_bounds("code-stop-rust-analyzer").is_none());
        assert!(cx.debug_bounds("code-start-rust-analyzer").is_some());
        let requests = fixture.transport.requests.lock().unwrap();
        let repeated: Vec<_> = requests
            .iter()
            .filter(|request| request.id == stopped.id)
            .collect();
        assert_eq!(repeated.len(), 2);
        assert_eq!(repeated[0], repeated[1]);
        drop(requests);
        tap(cx, "code-start-rust-analyzer");
        wait(cx, |cx| {
            !panel.read(cx).pending
                && !panel.read(cx).loading
                && panel
                    .read(cx)
                    .servers
                    .iter()
                    .any(|server| server.status == code::Status::Ready)
        });
        assert_eq!(fixture.settings(), settings);
        assert!(cx.debug_bounds("code-stop-rust-analyzer").is_some());
        assert!(cx.debug_bounds("code-start-rust-analyzer").is_none());
        fixture.bind(&owner, cx, false, Some(fixture.second));
        wait(cx, |cx| {
            selected(&owner, cx).read(cx).workspace.is_some()
                && !selected(&owner, cx).read(cx).loading
        });
        tap(cx, "code-tab-installed");
        assert!(cx.debug_bounds("code-server-rust-analyzer").is_some());
        assert!(cx.debug_bounds("code-edit-rust-analyzer").is_some());
        assert!(cx.debug_bounds("code-start-rust-analyzer").is_some());
        cx.update(|window, cx| {
            let rows = selected(&owner, cx).read(cx).rows();
            let row = rows
                .iter()
                .find(|row| row.server == "rust-analyzer")
                .unwrap();
            assert_eq!(row.languages, ["rust"]);
            assert!(row.candidate.as_ref().unwrap().startup.is_none());
            assert_eq!(
                window.find("code-start-rust-analyzer").label(),
                Some(crate::tr("code_start_unavailable").as_ref())
            );
        });
        let before = fixture.transport.requests.lock().unwrap().len();
        tap(cx, "code-start-rust-analyzer");
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), before);
        tap(cx, "code-details-rust-analyzer");
        assert!(cx.debug_bounds("code-detail-package").is_some());
        tap(cx, "code-details-uninstall");
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
        tap(cx, "code-consent-submit");
        wait(cx, |cx| {
            !selected(&owner, cx).read(cx).pending && !selected(&owner, cx).read(cx).loading
        });
        let Output::CodeInstallations(installs) = fixture.execute(Command::ListCodeInstallations)
        else {
            panic!("installations expected")
        };
        assert!(installs.iter().all(|install| install.receipt.is_none()));
        assert!(
            std::fs::read_to_string(fixture.root.join("main.rs"))
                .unwrap()
                .contains("fn main")
        );
        fixture.close(cx);
    }
}

#[gpui::test]
fn cancels_real_download_from_status_subscription(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let network = Network::new();
        network.held.store(true, Ordering::SeqCst);
        let fixture = Fixture::with_endpoint(remote, Some(network.endpoint.clone()));
        let (owner, cx) = fixture.mount(cx);
        let panel = cx.update(|_, cx| selected(&owner, cx));
        tap(cx, "code-install-rust-analyzer");
        tap(cx, "code-consent-submit");
        wait(cx, |cx| {
            network.hits.load(Ordering::SeqCst) > 0
                && panel
                    .read(cx)
                    .installations
                    .iter()
                    .any(|installation| installation.status == InstallStatus::Downloading)
        });
        assert!(cx.update(|_, cx| panel.read(cx).pending));
        assert!(cx.debug_bounds("code-install-rust-analyzer").is_some());
        tap(cx, "code-install-rust-analyzer");
        assert_eq!(
            fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| matches!(request.command, Command::InstallCodeServer { .. }))
                .count(),
            1
        );
        tap(cx, "code-cancel-rust-analyzer");
        wait(cx, |cx| {
            !panel.read(cx).pending
                && panel.read(cx).cancelling.is_none()
                && panel
                    .read(cx)
                    .installations
                    .iter()
                    .any(|installation| installation.status == InstallStatus::Cancelled)
        });
        assert_eq!(network.hits.load(Ordering::SeqCst), 1);
        assert!(
            fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| matches!(request.command, Command::CancelCodeInstall { .. }))
        );
        let Output::CodeInstallations(installs) = fixture.execute(Command::ListCodeInstallations)
        else {
            panic!("installations expected")
        };
        assert!(
            installs
                .iter()
                .all(|installation| installation.receipt.is_none())
        );
        assert!(cx.update(|_, cx| panel.read(cx).request.is_none()));
        fixture.close(cx);
    }
}

#[gpui::test]
fn retries_installation_and_rules_with_original_ids(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let network = Network::new();
        let fixture = Fixture::with_endpoint(remote, Some(network.endpoint.clone()));
        let (owner, cx) = fixture.mount(cx);
        let panel = cx.update(|_, cx| selected(&owner, cx));
        fixture.transport.drop_next.store(true, Ordering::SeqCst);
        tap(cx, "code-install-rust-analyzer");
        tap(cx, "code-consent-submit");
        wait(cx, |cx| {
            !panel.read(cx).pending
                && panel.read(cx).request.is_some()
                && panel
                    .read(cx)
                    .installations
                    .iter()
                    .any(|installation| installation.status == InstallStatus::Ready)
        });
        let request = request(&fixture);
        fixture.bind(&owner, cx, false, Some(fixture.second));
        wait(cx, |cx| !selected(&owner, cx).read(cx).loading);
        fixture.bind(&owner, cx, false, Some(fixture.worktree));
        tap(cx, "code-retry");
        wait(cx, |cx| {
            !panel.read(cx).pending && panel.read(cx).request.is_none() && !panel.read(cx).loading
        });
        assert_eq!(network.hits.load(Ordering::SeqCst), 1);
        let requests = fixture.transport.requests.lock().unwrap();
        let repeated: Vec<_> = requests
            .iter()
            .filter(|current| current.id == request.id)
            .collect();
        assert_eq!(repeated.len(), 2);
        assert_eq!(repeated[0], repeated[1]);
        drop(requests);
        fixture.transport.drop_next.store(true, Ordering::SeqCst);
        tap(cx, "code-update-rules");
        wait(cx, |cx| {
            !panel.read(cx).pending && panel.read(cx).request.is_some()
        });
        let rules_request = cx.update(|_, cx| panel.read(cx).request.clone().unwrap());
        assert_eq!(rules_request.command, Command::UpdateCodeRules);
        tap(cx, "code-retry");
        wait(cx, |cx| {
            !panel.read(cx).pending && !panel.read(cx).loading && panel.read(cx).request.is_none()
        });
        assert!(cx.update(|_, cx| panel.read(cx).rules.as_ref().unwrap().cached));
        let requests = fixture.transport.requests.lock().unwrap();
        let repeated: Vec<_> = requests
            .iter()
            .filter(|current| current.id == rules_request.id)
            .collect();
        assert_eq!(repeated.len(), 2);
        assert_eq!(repeated[0], repeated[1]);
        drop(requests);
        fixture.close(cx);
    }
}

#[gpui::test]
fn releases_a_confirmed_download_failure(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let network = Network::new();
        let endpoint = network.endpoint.clone();
        drop(network);
        let fixture = Fixture::with_endpoint(remote, Some(endpoint));
        let (owner, cx) = fixture.mount(cx);
        let panel = cx.update(|_, cx| selected(&owner, cx));
        tap(cx, "code-update-rules");
        wait(cx, |cx| !panel.read(cx).pending);
        cx.update(|_, cx| {
            assert!(panel.read(cx).request.is_none());
            assert_eq!(panel.read(cx).error, Some("code_failed"));
        });
        let request = fixture
            .transport
            .requests
            .lock()
            .unwrap()
            .iter()
            .find(|request| request.command == Command::UpdateCodeRules)
            .unwrap()
            .clone();
        let client = sailry_client::Client::new(fixture.transport.clone());
        let sailry_protocol::RequestOutcome::Completed(result) =
            fixture.runtime.block_on(client.outcome(&request)).unwrap()
        else {
            panic!("canonical completion expected")
        };
        assert_eq!(
            result.unwrap_err().code,
            sailry_protocol::ErrorCode::Unavailable
        );
        fixture.close(cx);
    }
}
