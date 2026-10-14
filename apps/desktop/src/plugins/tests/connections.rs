use super::*;
use crate::shell::Shell;
use sailry_protocol::{Output, plugin};

mod databases;
mod ssh;

fn landing_width(visual: &mut VisualTestContext, prefix: &str) {
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let button = visual
        .debug_bounds(Box::leak(format!("{prefix}-new").into_boxed_str()))
        .unwrap();
    let list = visual
        .debug_bounds(Box::leak(
            format!("{prefix}-saved-connections").into_boxed_str(),
        ))
        .unwrap();
    assert_eq!(button.left(), list.left());
    assert_eq!(button.right(), list.right());
    assert!(button.size.width > px(200.));
}

fn saved_rows(fixture: &Fixture, name: &str, count: usize, port: u16) -> Vec<&'static str> {
    let path = fixture.directory.path().join("connections.sqlite3");
    if name == "databases" {
        drop(rusqlite::Connection::open(&path).unwrap());
    }
    (0..count)
        .map(|index| {
            let label = format!("Connection {index:02}");
            let selector = if name == "databases" {
                let Output::DatabaseProfile(profile) = fixture.execute(Command::SaveDatabase {
                    profile: sailry_protocol::database::Profile {
                        id: format!("00000000-0000-0000-0000-{:012}", index + 1)
                            .parse()
                            .unwrap(),
                        revision: 0,
                        name: label,
                        connection: sailry_protocol::database::Connection::Sqlite {
                            path: path.to_str().unwrap().into(),
                        },
                        read_only: false,
                        sharing: None,
                    },
                    expected_revision: 0,
                    password: None,
                }) else {
                    panic!("database profile expected");
                };
                format!("db-open-{}", profile.id)
            } else {
                let Output::SshProfile(profile) = fixture.execute(Command::SaveSsh {
                    profile: sailry_protocol::ssh::Profile {
                        id: format!("00000000-0000-0000-0000-{:012}", index + 1)
                            .parse()
                            .unwrap(),
                        revision: 0,
                        name: label,
                        host: "127.0.0.1".into(),
                        port,
                        username: "fixture".into(),
                        authentication: sailry_protocol::ssh::Authentication::Password,
                        host_key: None,
                        sharing: None,
                    },
                    expected_revision: 0,
                    credential: Some(sailry_protocol::ssh::Credential::Password {
                        password: sailry_protocol::Secret::new("isolated-ssh-password".into()),
                    }),
                }) else {
                    panic!("SSH profile expected");
                };
                format!("ssh-open-{}", profile.id)
            };
            &*Box::leak(selector.into_boxed_str())
        })
        .collect()
}

#[gpui::test]
fn cards_keep_spacing_and_scroll_to_last(cx: &mut TestAppContext) {
    init(cx);
    let mut row_height = None;
    for remote in [false, true] {
        for count in [3, 12] {
            for name in ["databases", "ssh"] {
                let fixture = Fixture::new(remote);
                let package = install(&fixture, name);
                let peer = (name == "ssh").then(|| {
                    fixture.runtime.block_on(crate::ssh_fixture::Server::start(
                        fixture.directory.path().to_path_buf(),
                        79,
                    ))
                });
                let rows = saved_rows(
                    &fixture,
                    name,
                    count,
                    peer.as_ref().map_or(0, |peer| peer.port),
                );
                let prefix = if name == "databases" { "db" } else { "ssh" };
                let (shell, panel, visual) = mount(&fixture, remote, name, cx);
                shown(&panel, visual, rows.last().unwrap());
                let icon: &'static str =
                    Box::leak(format!("empty-icon-{prefix}_workspace_empty").into_boxed_str());
                let button: &'static str = Box::leak(format!("{prefix}-new").into_boxed_str());
                let handle = visual.update(|window, _| window.window_handle());
                for height in [940., 400.] {
                    visual.simulate_window_resize(handle, size(px(1440.), px(height)));
                    visual.run_until_parked();
                    visual.update(|window, cx| window.draw(cx).clear(cx));
                    // Kit's overlay measures the viewport; Bounds inside the scrollable
                    // content measures the entire list after materialization.
                    let viewport = visual.debug_bounds("scrollbar-overlay").unwrap();
                    let heading = visual.debug_bounds(icon).unwrap();
                    let action = visual.debug_bounds(button).unwrap();
                    assert!(viewport.size.height <= px(320.));
                    assert!(viewport.top() >= action.bottom());
                    assert!(viewport.bottom() <= px(height));
                    assert_eq!(viewport.left(), action.left());
                    assert_eq!(viewport.right(), action.right());
                    visual.simulate_event(ScrollWheelEvent {
                        position: viewport.center(),
                        delta: ScrollDelta::Pixels(point(px(0.), px(40_000.))),
                        touch_phase: TouchPhase::Moved,
                        ..Default::default()
                    });
                    visual.run_until_parked();
                    visual.update(|window, cx| window.draw(cx).clear(cx));
                    let first = visual.debug_bounds(rows[0]).unwrap();
                    let expected = *row_height.get_or_insert(first.size.height);
                    assert_eq!(first.size.height, expected);
                    assert!(first.top() >= viewport.top());
                    for pair in rows.windows(2) {
                        let before = visual.debug_bounds(pair[0]).unwrap();
                        let after = visual.debug_bounds(pair[1]).unwrap();
                        assert_eq!(after.size.height, expected);
                        let gap = after.top() - before.bottom();
                        assert!(
                            (gap - px(8.)).abs() < px(1.),
                            "{name} count {count} height {height}: gap {gap:?}"
                        );
                    }
                    let last = visual.debug_bounds(rows[count - 1]).unwrap();
                    if count == 12 {
                        assert!(last.bottom() > viewport.bottom());
                    }
                    visual.simulate_event(ScrollWheelEvent {
                        position: first.center(),
                        delta: ScrollDelta::Pixels(point(px(0.), px(-40_000.))),
                        touch_phase: TouchPhase::Moved,
                        ..Default::default()
                    });
                    visual.run_until_parked();
                    visual.update(|window, cx| window.draw(cx).clear(cx));
                    let last = visual.debug_bounds(rows[count - 1]).unwrap();
                    assert!(last.top() >= viewport.top());
                    assert!(last.bottom() <= viewport.bottom() + px(1.));
                    assert_eq!(visual.debug_bounds(icon).unwrap(), heading);
                    assert_eq!(visual.debug_bounds(button).unwrap(), action);
                    assert_eq!(visual.debug_bounds("scrollbar-overlay").unwrap(), viewport);
                }
                let last = rows[count - 1];
                tap(visual, last);
                shown(
                    &panel,
                    visual,
                    if name == "databases" {
                        "db-page"
                    } else {
                        "ssh-host-key"
                    },
                );
                assert!(
                    fixture
                        .transport
                        .requests
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|request| {
                            match &request.command {
                                Command::BrowseDatabase { profile, .. } => {
                                    format!("db-open-{profile}") == last
                                }
                                Command::OpenSshTerminal { profile, .. } => {
                                    format!("ssh-open-{profile}") == last
                                }
                                _ => false,
                            }
                        })
                );
                if let Some(peer) = peer {
                    assert_eq!(peer.authentication.load(Ordering::SeqCst), 0);
                    fixture.runtime.block_on(peer.close());
                }
                visual.update(|window, _| window.remove_window());
                drop(panel);
                drop(shell);
                drop(package);
                fixture.close();
            }
        }
    }
}

fn install(fixture: &Fixture, name: &str) -> plugin::Info {
    let path = fixture.directory.path().join("project").join(name);
    fixture::copy_package(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugins")
            .join(name),
        &path,
    );
    for file in ["view.js", "editor-view.js"] {
        let file = path.join("dev.sailry.platform/desktop").join(file);
        let source = std::fs::read_to_string(&file)
            .unwrap()
            .replace("{Button,", "{Button as KitButton,")
            .replace(
                "new Tab().label(text.db_sql_tab)",
                "new Tab().child(Anchor.new('db-sql-tab').child(div().child(text.db_sql_tab)))",
            )
            .replace(
                "new Tab().label(text[`ssh_${kind}`])",
                "new Tab().child(Anchor.new(`ssh-auth-${kind}`).child(div().child(text[`ssh_${kind}`])))",
            )
            .replace(
                "new Tab().label(text.db_history)",
                "new Tab().child(Anchor.new('db-history-tab').child(div().child(text.db_history)))",
            )
            // Bounds observe scripted labels without changing their layout or callbacks.
            .replace(
                ".child(div().id(`db-log-sql-${index}`)",
                ".child(div().id(`db-log-sql-${index}`).relative().child(Bounds.new(`db-log-sql-${index}`))",
            )
            // The registered Button.label value is opaque in Kit's debug tree.
            // Measure the selected filename on the same native button without
            // replacing its label, callback, or credential-file workflow.
            .replace(
                ".label(key.file??text.ssh_key_choose)",
                ".label(key.file??text.ssh_key_choose).children(key.file?[Bounds.new(key.file)]:[])",
            )
            .replace(".id('db-saved-connections')", ".id('db-saved-connections').relative().child(Bounds.new('db-saved-connections'))")
            .replace(".id('ssh-saved-connections')", ".id('ssh-saved-connections').relative().child(Bounds.new('ssh-saved-connections'))");
        // Anchors expose the real Kit button hit targets without replacing callbacks.
        std::fs::write(file, format!("import {{Anchor,Bounds}} from 'sailry/test';\n{source}\nfunction Button(id) {{return new KitButton(id).relative().child(Bounds.new(id));}}\n")).unwrap();
    }
    let Output::Plugin(current) = fixture.execute(Command::ReadPlugin { name: name.into() }) else {
        panic!("package expected");
    };
    let Output::Plugin(info) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: name.into(),
        name: name.into(),
        expected_revision: current.summary.revision,
        expected_digest: None,
    }) else {
        panic!("package expected");
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    info
}

fn mount<'a>(
    fixture: &Fixture,
    remote: bool,
    name: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<Shell>, Entity<Panel>, &'a mut VisualTestContext) {
    cx.update(|cx| cx.set_global(fixture.services(remote)));
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        owner = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = owner.unwrap();
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1440.), px(940.)));
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
            shell.live.as_mut().unwrap().select(fixture.node.id(), cx)
        })
    });
    wait(visual, |cx| {
        shell
            .read(cx)
            .extension_entries(cx)
            .iter()
            .any(|entry| entry.package.name == name)
    });
    let selector = shell.read_with(visual, |shell, cx| {
        shell
            .extension_entries(cx)
            .into_iter()
            .find(|entry| entry.package.name == name)
            .unwrap()
            .selector()
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("shell-navigation").is_some());
    tap(visual, &selector);
    wait(visual, |cx| {
        shell
            .read(cx)
            .extensions
            .as_ref()
            .is_some_and(|extensions| extensions.panel.is_some())
    });
    let panel = shell.read_with(visual, |shell, _| {
        shell.extensions.as_ref().unwrap().panel.clone().unwrap()
    });
    wait(visual, |cx| {
        let panel = panel.read(cx);
        panel.connected && panel.selected.is_some()
    });
    // Keep Shell routing and mount the same Node through the observable fixture transport.
    visual.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            let package = panel.selected.clone().unwrap();
            panel.back(cx);
            panel.binding.client = fixture.binding.client.clone();
            panel.open(package, window, cx);
        })
    });
    shown(
        &panel,
        visual,
        if name == "databases" {
            "db-landing"
        } else {
            "ssh-landing"
        },
    );
    assert!(visual.debug_bounds("shell-navigation").is_none());
    assert!(visual.debug_bounds("header-sidebar-toggle").is_none());
    assert_eq!(
        visual.debug_bounds("main-content").unwrap().left(),
        px(crate::preview::RAIL_WIDTH),
    );
    no_session_navigation(visual);
    assert_eq!(
        panel.read_with(visual, |panel, _| panel.binding.client.target()),
        fixture.node.id()
    );
    assert!(panel.read_with(visual, |panel, _| panel.binding.worktree.is_none()));
    (shell, panel, visual)
}

#[track_caller]
fn no_session_navigation(visual: &mut VisualTestContext) {
    for selector in [
        "new-conversation",
        "hosts-heading",
        "projects-heading",
        "sidebar-active",
    ] {
        assert!(
            visual.debug_bounds(selector).is_none(),
            "unexpected conversation navigation: {selector}",
        );
    }
}

#[track_caller]
fn shown(panel: &Entity<Panel>, visual: &mut VisualTestContext, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let selector: &'static str = Box::leak(needle.to_owned().into_boxed_str());
    loop {
        visual.run_until_parked();
        let state = visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            snapshot(panel, cx)
        });
        if state.contains(needle) || visual.debug_bounds(selector).is_some() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "connection view deadline: {needle}; {state}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[track_caller]
fn enabled(panel: &Entity<Panel>, visual: &mut VisualTestContext, id: &str) {
    let selector = format!("\"{id}\"");
    wait(visual, |cx| {
        let state = snapshot(panel, cx);
        if state.lines().any(|line| {
            line.contains("module_component sailry/ui.Modal")
                && line.contains("(\"open\", Bool(true))")
        }) {
            return false;
        }
        if let Some(line) = state.lines().find(|line| {
            line.contains("module_component sailry/ui.SelectableRow")
                && line.contains(&selector)
                && line.contains("(\"variant\", Str(\"card\"))")
        }) {
            return !line.contains("(\"disabled\", Bool(true))");
        }
        let Some(offset) = state.find(&selector) else {
            return false;
        };
        state[..offset]
            .lines()
            .rev()
            .find(|line| line.trim_start().starts_with("Button "))
            .is_some_and(|line| !line.contains(":disabled[Bool(true)]"))
    });
}
#[track_caller]
fn tap(visual: &mut VisualTestContext, id: &str) {
    if visual.update(|window, cx| window.has_active_dialog(cx)) {
        std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
    }
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let id: &'static str = Box::leak(id.to_owned().into_boxed_str());
    let bounds = visual
        .debug_bounds(id)
        .unwrap_or_else(|| panic!("missing {id}"));
    visual.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
}
#[track_caller]
fn input(visual: &mut VisualTestContext, id: &str, value: &str) {
    tap(visual, id);
    let retained = visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        window.focused_input(cx)
    });
    visual.simulate_keystrokes("secondary-a");
    visual.simulate_input(value);
    visual.run_until_parked();
    let (same_input, matches, retained_len, current_len, focus) = visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        let current = window.focused_input(cx);
        let retained_value = retained.as_ref().map(|input| input.value(cx));
        let current_value = current.as_ref().map(|input| input.value(cx));
        (
            retained.is_some() && retained == current,
            retained_value.as_deref() == Some(value),
            retained_value.as_ref().map(|value| value.chars().count()),
            current_value.as_ref().map(|value| value.chars().count()),
            window.focused(cx),
        )
    });
    let selector: &'static str = Box::leak(id.to_owned().into_boxed_str());
    assert!(
        same_input && (id.starts_with("secret-") || matches),
        "connection input mismatch: id={id}, bounds={:?}, same_input={same_input}, matches={matches}, retained_len={retained_len:?}, current_len={current_len:?}, expected_len={}, focus={focus:?}",
        visual.debug_bounds(selector),
        value.chars().count(),
    );
}

fn action(panel: &Entity<Panel>, visual: &mut VisualTestContext, prefix: &str, action: &str) {
    wait(visual, |cx| {
        !snapshot(panel, cx).lines().any(|line| {
            line.contains("module_component sailry/ui.Modal")
                && line.contains("(\"open\", Bool(true))")
        })
    });
    tap(visual, &format!("{prefix}-connections-menu"));
    shown(panel, visual, "connection-choice-0");
    tap(visual, "connection-choice-0");
    let selector = format!("connection-action-{action}");
    shown(panel, visual, &selector);
    tap(visual, &selector);
}
fn state(fixture: &Fixture) -> sailry_protocol::Snapshot {
    let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
        panic!("snapshot expected");
    };
    snapshot
}
