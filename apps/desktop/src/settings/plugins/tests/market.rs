use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex, atomic::AtomicBool},
};

struct Server {
    endpoint: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::spawn(move || {
            let manifest = serde_json::json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name":"portable", "version":"1.0.0", "description":"Portable skills without a desktop extension"});
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            for (path, body) in [("repo/plugins/portable/plugin.json", manifest.to_string()), ("repo/plugins/portable/skills/review/SKILL.md", "---\nname: review\ndescription: Review project changes\n---\nReview the changes\n".into()), ("repo/plugins/portable/mcp.json", serde_json::json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json", "mcpServers":{"remote":{"type":"streamable-http", "url":"https://example.invalid/mcp"}}}).to_string())] {
                zip.start_file(path, zip::write::SimpleFileOptions::default()).unwrap(); zip.write_all(body.as_bytes()).unwrap();
            }
            let archive = zip.finish().unwrap().into_inner();
            for stream in listener.incoming() {
                if stopped.load(Ordering::SeqCst) {
                    break;
                }
                let mut stream = stream.unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 2048];
                while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                    let count = stream.read(&mut buffer).unwrap();
                    if count == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..count]);
                }
                let request = String::from_utf8(request).unwrap();
                let path = request.split_whitespace().nth(1).unwrap();
                captured.lock().unwrap().push(path.to_owned());
                let body = if path.starts_with("/api/v1/plugins?") {
                    assert!(path.contains("protocol=agent-plugins"));
                    serde_json::json!({"data":[{"slug":"portable", "name":"portable", "description":"Portable review skill", "repoUrl":"https://github.com/fixture/plugins", "protocols":["agent-plugins"]}],"meta":{"page":1,"total_pages":1}}).to_string().into_bytes()
                } else if path == "/api/v1/plugins/portable" {
                    serde_json::json!({"data":{"repoUrl":"https://github.com/fixture/plugins", "pluginPath":"plugins/portable", "manifests":{"agent-plugins":{"path":"plugin.json", "raw":manifest.to_string()}}}}).to_string().into_bytes()
                } else if path == "/repos/fixture/plugins/commits/HEAD" {
                    "a".repeat(40).into_bytes()
                } else if path == format!("/fixture/plugins/zip/{}", "a".repeat(40)) {
                    archive.clone()
                } else {
                    panic!("unexpected catalog fixture request: {path}");
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                let _ = stream.write_all(&body);
            }
        });
        Self {
            endpoint: format!("http://{address}"),
            requests,
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = std::net::TcpStream::connect(self.endpoint.trim_start_matches("http://"));
        self.thread.take().unwrap().join().unwrap();
    }
}

#[gpui::test]
fn admitted_install_shows_disabled_progress(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let server = Server::new();
        let fixture = Fixture::with_skill_source(remote, &server.endpoint);
        let (_, visual) = fixture.mount(cx);
        tap(visual, "plugins-market-tab");
        shown(visual, "market-install-files", true);
        tap(visual, "plugins-third-party");
        shown(visual, "market-install-portable", true);
        fixture.transport.mode.store(2, Ordering::SeqCst);
        tap(visual, "market-install-portable");
        wait(visual, |_| fixture.transport.entered.is_cancelled());
        let requests = fixture.transport.requests.lock().unwrap().len();
        tap(visual, "plugin-download-submit");
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), requests);
        fixture.transport.release.cancel();
        shown(visual, "plugin-download-submit", false);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        assert_eq!(fixture.plugins()[0].revision, 1);
        fixture.close(visual);
    }
}

#[gpui::test]
fn previews_without_installing(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let server = Server::new();
        let fixture = Fixture::with_skill_source(remote, &server.endpoint);
        let before = fixture.public_packages(false);
        let (_, visual) = fixture.mount(cx);
        tap(visual, "plugins-market-tab");
        tap(visual, "market-details-gomoku");
        shown(visual, "plugin-info-version", true);
        shown(visual, "plugin-details-configure", false);
        assert_eq!(fixture.public_packages(false), before);
        assert!(fixture.transport.requests.lock().unwrap().is_empty());
        assert!(server.requests.lock().unwrap().is_empty());
        visual.simulate_keystrokes("escape");
        tap(visual, "market-details-files");
        shown(visual, "plugin-tool-read_file", true);
        visual.simulate_keystrokes("escape");
        tap(visual, "plugins-third-party");
        tap(visual, "market-details-portable");
        shown(visual, "plugin-skill-review", true);
        shown(visual, "plugin-server-remote", true);
        shown(visual, "plugin-authorize-remote", false);
        shown(visual, "plugin-info-version", true);
        shown(visual, "plugin-details-configure", false);
        assert_eq!(fixture.public_packages(false), before);
        assert!(fixture.transport.requests.lock().unwrap().is_empty());
        assert!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|path| path == "/api/v1/plugins/portable")
        );
        visual.simulate_keystrokes("escape");
        shown(visual, "plugin-live-details", false);
        fixture.close(visual);
    }
}

#[gpui::test]
fn details_keep_their_node_and_cancel_on_close(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        for close in [false, true] {
            let server = Server::new();
            let fixture = Fixture::with_skill_source(remote, &server.endpoint);
            let before = fixture.public_packages(false);
            let (owner, visual) = fixture.mount(cx);
            tap(visual, "plugins-market-tab");
            tap(visual, "plugins-third-party");
            shown(visual, "market-details-portable", true);
            wait(visual, |cx| {
                owner
                    .read(cx)
                    .plugin_catalog
                    .metadata
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .settled()
            });
            fixture.transport.read_mode.store(2, Ordering::SeqCst);
            tap(visual, "market-details-portable");
            wait(visual, |_| fixture.transport.entered.is_cancelled());
            shown(visual, "plugin-details-skeleton", true);
            if close {
                visual.simulate_keystrokes("escape");
                wait(visual, |_| fixture.transport.read_cancelled.is_cancelled());
                tap(visual, "plugins-official");
                tap(visual, "market-details-files");
                shown(visual, "plugin-tool-read_file", true);
            } else {
                fixture.bind(&owner, visual, true);
            }
            fixture.transport.release.cancel();
            if close {
                draw(visual);
                shown(visual, "plugin-details-skeleton", false);
                shown(visual, "plugin-skill-review", false);
                shown(visual, "plugin-tool-read_file", true);
                assert!(
                    !server
                        .requests
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|path| path == "/api/v1/plugins/portable")
                );
            } else {
                shown(visual, "plugin-skill-review", true);
                shown(visual, "plugin-details-skeleton", false);
                assert!(
                    server
                        .requests
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|path| path == "/api/v1/plugins/portable")
                );
            }
            assert_eq!(fixture.public_packages(false), before);
            assert!(fixture.transport.requests.lock().unwrap().is_empty());
            visual.simulate_keystrokes("escape");
            fixture.close(visual);
        }
    }
}

#[gpui::test]
fn installs_portable_packages_from_catalog(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let server = Server::new();
        let fixture = Fixture::with_skill_source(remote, &server.endpoint);
        let (owner, visual) = fixture.mount(cx);
        tap(visual, "plugins-market-tab");
        shown(visual, "market-install-files", true);
        let icon = visual.debug_bounds("market-icon-files").unwrap();
        let text = visual.debug_bounds("market-summary-files").unwrap();
        let install = visual.debug_bounds("market-install-files").unwrap();
        assert!(icon.right() <= text.left());
        assert!(text.right() < install.left());
        assert!((icon.center().y - text.center().y).abs() < px(1.));
        assert!((icon.center().y - install.center().y).abs() < px(1.));
        assert!(server.requests.lock().unwrap().is_empty());
        shown(visual, "market-install-portable", false);
        tap(visual, "plugins-third-party");
        shown(visual, "market-install-portable", true);
        shown(visual, "market-install-project-summary", false);
        let requests = server.requests.lock().unwrap().len();
        for _ in 0..10 {
            draw(visual);
        }
        assert_eq!(server.requests.lock().unwrap().len(), requests);
        tap(visual, "market-install-portable");
        wait(visual, |_| {
            fixture
                .plugins()
                .iter()
                .any(|plugin| plugin.name == "portable")
        });
        wait(visual, |cx| {
            owner
                .read(cx)
                .plugin_catalog
                .packages
                .iter()
                .any(|plugin| plugin.name == "portable")
        });
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        let Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
            name: "portable".into(),
        }) else {
            panic!("plugin expected")
        };
        assert!(info.extension.is_none());
        assert!(info.icon.is_none());
        assert_eq!(info.skills[0].name, "review");
        assert!(matches!(
            info.origin,
            Some(sailry_protocol::plugin::Origin::Online { .. })
        ));
        tap(visual, "plugins-manage-tab");
        updates::ready(&owner, visual, "portable", 1);
        let requests = fixture.transport.requests.lock().unwrap().len();
        let downloads = server.requests.lock().unwrap().len();
        menu(visual, "plugin-menu-portable", 1);
        wait(visual, |cx| {
            !owner.read(cx).plugin_catalog.updates.checking()
        });
        assert!(!visual.did_prompt_for_paths());
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), requests);
        assert!(server.requests.lock().unwrap().len() > downloads);
        assert_eq!(fixture.plugins()[0].revision, 1);
        shown(visual, "plugin-update-portable", false);
        tap(visual, "plugin-menu-portable");
        visual.simulate_keystrokes("escape");
        tap(visual, "plugin-details-portable");
        shown(visual, "plugin-skill-review", true);
        visual.simulate_keystrokes("escape");
        tap(visual, "plugins-market-tab");
        tap(visual, "plugins-official");
        shown(visual, "market-install-gomoku", true);
        tap(visual, "market-install-gomoku");
        wait(visual, |_| fixture.plugins().len() == 2);
        let Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
            name: "gomoku".into(),
        }) else {
            panic!("plugin expected")
        };
        assert!(info.icon.is_none());
        let glyph = crate::plugins::emblem::glyph(&info).unwrap();
        assert_eq!(
            glyph,
            &sailry_protocol::plugin::desktop::Icon::Name("reicon:newicons/grid2".into())
        );
        fixture.close(visual);
    }
}

#[gpui::test]
fn downloads_and_retries_the_same_admitted_install(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let server = Server::new();
        let fixture = Fixture::with_skill_source(remote, &server.endpoint);
        let (owner, visual) = fixture.mount(cx);
        tap(visual, "plugins-download");
        input(visual, "plugin-source-0", "fixture/plugins");
        input(visual, "plugin-source-2", "plugins/portable");
        fixture.transport.mode.store(1, Ordering::SeqCst);
        tap(visual, "plugin-download-submit");
        wait(visual, |_| fixture.plugins().len() == 1);
        crate::feedback::tests::shown(visual);
        let id = *fixture.transport.requests.lock().unwrap().last().unwrap();
        fixture.bind(&owner, visual, true);
        tap(visual, "plugin-download-submit");
        wait(visual, |_| {
            fixture.transport.requests.lock().unwrap().len() >= 2
        });
        assert_eq!(
            *fixture.transport.requests.lock().unwrap().last().unwrap(),
            id
        );
        assert_eq!(fixture.plugins()[0].revision, 1);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        fixture.close(visual);
    }
}

#[gpui::test]
fn separates_card_details_and_toggles(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        for package in &fixture.initial_packages {
            if !["databases", "goals", "web-search"].contains(&package.name.as_str()) {
                fixture.execute(Command::RemovePlugin {
                    name: package.name.clone(),
                    expected_revision: package.revision,
                });
            }
        }
        let (owner, visual) = fixture.mount(cx);
        shown(visual, "plugin-details-web-search", true);
        shown(visual, "empty-plugins_none", false);
        wait(visual, |cx| {
            owner
                .read(cx)
                .plugin_catalog
                .metadata
                .as_ref()
                .unwrap()
                .read(cx)
                .settled()
        });
        draw(visual);
        let first = visual.debug_bounds("plugin-details-databases").unwrap();
        let second = visual.debug_bounds("plugin-details-goals").unwrap();
        assert!(second.origin.x > first.origin.x);
        assert_eq!(first.origin.y, second.origin.y);
        let icon = visual.debug_bounds("plugin-icon-databases").unwrap();
        let text = visual.debug_bounds("plugin-summary-databases").unwrap();
        let toggle = visual.debug_bounds("plugin-toggle-databases").unwrap();
        assert!(icon.right() <= text.left());
        assert!(text.right() < toggle.left());
        assert!((icon.center().y - text.center().y).abs() < px(1.));
        assert!((icon.center().y - toggle.center().y).abs() < px(1.));
        tap(visual, "plugin-toggle-databases");
        wait(visual, |cx| !owner.read(cx).plugin_catalog.busy());
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        tap(visual, "plugin-details-databases");
        shown(visual, "plugin-tool-database_query", true);
        visual.simulate_keystrokes("escape");
        fixture.close(visual);
    }
}

#[gpui::test]
fn clearing_search_restores_catalog(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let server = Server::new();
        let fixture = Fixture::with_skill_source(remote, &server.endpoint);
        let (_, visual) = fixture.mount(cx);
        tap(visual, "plugins-market-tab");
        shown(visual, "market-install-files", true);
        input(visual, "plugins-search-input", "missing-plugin-fixture");
        tap(visual, "plugins-search");
        shown(visual, "empty-plugins_none", true);
        tap(visual, "plugins-search-input");
        visual.simulate_keystrokes("secondary-a backspace");
        shown(visual, "market-install-files", true);
        shown(visual, "empty-plugins_none", false);

        tap(visual, "plugins-third-party");
        shown(visual, "market-install-portable", true);
        input(visual, "plugins-search-input", "portable");
        tap(visual, "plugins-search");
        wait(visual, |_| {
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|path| path.contains("q=portable"))
        });
        shown(visual, "market-install-portable", true);
        let requests = server.requests.lock().unwrap().len();
        tap(visual, "plugins-search-input");
        visual.simulate_keystrokes("secondary-a backspace");
        wait(visual, |_| server.requests.lock().unwrap().len() > requests);
        let request = server.requests.lock().unwrap().last().unwrap().clone();
        let url = url::Url::parse(&format!("{}{}", server.endpoint, request)).unwrap();
        assert!(
            url.query_pairs()
                .any(|(key, value)| key == "q" && value.is_empty())
        );
        assert!(
            url.query_pairs()
                .any(|(key, value)| key == "page" && value == "1")
        );
        shown(visual, "market-install-portable", true);
        fixture.close(visual);
    }
}

#[gpui::test]
fn remains_interactive_while_loading(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        for action in ["search", "source", "clear", "complete"] {
            let server = Server::new();
            let fixture = Fixture::with_skill_source(remote, &server.endpoint);
            let (_, visual) = fixture.mount(cx);
            tap(visual, "plugins-market-tab");
            shown(visual, "market-install-files", true);
            fixture.transport.catalog_hold.store(1, Ordering::SeqCst);
            tap(visual, "plugins-third-party");
            assert!(visual.debug_bounds("plugin-market-skeleton").is_some());
            assert!(visual.debug_bounds("market-install-files").is_none());
            wait(visual, |_| fixture.transport.catalog_entered.is_cancelled());
            shown(visual, "plugin-market-skeleton", true);
            shown(visual, "market-install-files", false);
            input(visual, "plugins-search-input", "files");
            if action == "source" {
                tap(visual, "plugins-official");
                shown(visual, "market-install-files", true);
                shown(visual, "market-install-portable", false);
            } else if action == "search" {
                tap(visual, "plugins-search");
                shown(visual, "market-install-portable", true);
                assert!(
                    server
                        .requests
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|path| path.contains("q=files"))
                );
            } else if action == "clear" {
                visual.simulate_keystrokes("secondary-a backspace");
                shown(visual, "market-install-portable", true);
            } else {
                fixture.transport.release.cancel();
                shown(visual, "market-install-portable", true);
            }
            wait(visual, |_| {
                fixture.transport.catalog_cancelled.is_cancelled()
            });
            shown(visual, "plugin-market-skeleton", false);
            fixture.close(visual);
        }
    }
}

#[gpui::test]
fn failed_source_switch_does_not_restore_previous_results(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let server = Server::new();
        let fixture = Fixture::with_skill_source(remote, &server.endpoint);
        let (_, visual) = fixture.mount(cx);
        tap(visual, "plugins-market-tab");
        shown(visual, "market-install-files", true);
        fixture.transport.catalog_hold.store(2, Ordering::SeqCst);
        tap(visual, "plugins-third-party");
        shown(visual, "plugins-market-retry", true);
        shown(visual, "plugin-market-skeleton", false);
        shown(visual, "market-install-files", false);
        crate::feedback::tests::shown(visual);
        tap(visual, "plugins-market-retry");
        shown(visual, "market-install-portable", true);
        shown(visual, "plugins-market-retry", false);
        shown(visual, "market-install-files", false);
        fixture.close(visual);
    }
}

#[gpui::test]
fn failed_refresh_retains_catalog_and_query(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (_, visual) = fixture.mount(cx);
        tap(visual, "plugins-market-tab");
        shown(visual, "market-install-files", true);
        input(visual, "plugins-search-input", "files");
        fixture.transport.catalog_hold.store(2, Ordering::SeqCst);
        tap(visual, "plugins-search");
        crate::feedback::tests::shown(visual);
        assert_eq!(
            visual.update(crate::feedback::tests::summary),
            tr("plugins_market_failed")
        );
        shown(visual, "market-install-files", true);
        shown(visual, "plugin-market-skeleton", false);
        shown(visual, "plugins-market-retry", true);
        tap(visual, "plugins-search-input");
        visual.simulate_keystrokes("secondary-a secondary-c");
        assert_eq!(
            visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            "files"
        );
        tap(visual, "plugins-market-retry");
        shown(visual, "plugins-market-retry", false);
        shown(visual, "market-install-files", true);
        fixture.close(visual);
    }
}

#[gpui::test]
fn releases_loading_when_rebinding(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        fixture.transport.catalog_hold.store(1, Ordering::SeqCst);
        tap(visual, "plugins-market-tab");
        wait(visual, |_| fixture.transport.catalog_entered.is_cancelled());
        shown(visual, "plugin-market-skeleton", true);
        fixture.bind(&owner, visual, true);
        wait(visual, |_| {
            fixture.transport.catalog_cancelled.is_cancelled()
        });
        shown(visual, "market-install-files", true);
        shown(visual, "plugin-market-skeleton", false);
        fixture.close(visual);
    }
}
