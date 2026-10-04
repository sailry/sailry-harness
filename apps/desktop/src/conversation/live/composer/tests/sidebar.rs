use super::*;
use crate::conversation::live::tests::fixture;
use sailry_protocol::{connection::Resource, database, plugin, ssh};

#[gpui::test]
fn connection_controls_follow_the_available_width(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected");
        };
        let mut provider = snapshot.providers[0].clone();
        let mut alternate = provider.models[0].clone();
        alternate.id = "sidebar-alternate".into();
        provider.models.push(alternate);
        fixture.execute(Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        });
        let database = sailry_protocol::DatabaseId::new();
        let path = fixture.directory.path().join("sidebar.sqlite3");
        drop(rusqlite::Connection::open(&path).unwrap());
        fixture.execute(Command::SaveDatabase {
            profile: database::Profile {
                id: database,
                revision: 0,
                name: "Sidebar database".into(),
                connection: database::Connection::Sqlite {
                    path: path.to_str().unwrap().into(),
                },
                read_only: false,
                sharing: None,
            },
            expected_revision: 0,
            password: None,
        });
        let ssh = sailry_protocol::SshId::new();
        fixture.execute(Command::SaveSsh {
            profile: ssh::Profile {
                id: ssh,
                revision: 0,
                name: "Sidebar SSH".into(),
                host: "localhost".into(),
                port: 22,
                username: "fixture".into(),
                authentication: ssh::Authentication::Agent,
                host_key: None,
                sharing: None,
            },
            expected_revision: 0,
            credential: Some(ssh::Credential::Agent),
        });
        let Output::Snapshot(before) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected");
        };
        for (package, assistant, resource) in [
            ("databases", "database", Resource::Database(database)),
            ("ssh", "ssh", Resource::Ssh(ssh)),
        ] {
            let Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
                name: package.into(),
            }) else {
                panic!("package expected");
            };
            let assistant = plugin::conversation::Binding {
                package: info.summary.reference(),
                id: assistant.into(),
            };
            let mut binding = fixture.binding.clone();
            binding.project = None;
            binding.worktree = None;
            let mut owner = None;
            let (_, visual) = cx.add_window_view(|window, cx| {
                let view = cx.new(|cx| {
                    View::for_assistant(
                        binding,
                        None,
                        assistant.clone(),
                        Some(resource),
                        window,
                        cx,
                    )
                });
                owner = Some(view.clone());
                Root::new(cx.new(|_| fixture::Harness(view)), window, cx)
            });
            let view = owner.unwrap();
            wait(visual, |cx| {
                let view = view.read(cx);
                view.connected() && view.config.is_some() && view.contributions.read(cx).ready(cx)
            });
            tap(visual, "live-chat-input");
            visual.simulate_input("Keep this connection draft 中文");
            let handle = visual.update(|window, _| window.window_handle());
            for width in [280., 360., 520., 900.] {
                visual.simulate_window_resize(handle, size(px(width), px(820.)));
                wait(visual, |cx| {
                    view.read(cx).compact_composer == (width < 680.)
                        && view.read(cx).icon_context == (width < 440.)
                });
                visual.update(|window, cx| window.draw(cx).clear(cx));
                let compact = width < 680.;
                assert_eq!(visual.debug_bounds("live-chat-model").is_some(), !compact);
                assert_eq!(
                    visual.debug_bounds("live-chat-permission").is_some(),
                    !compact
                );
                assert!(visual.debug_bounds("live-chat-mode").is_none());
                assert_eq!(visual.debug_bounds("composer-settings").is_some(), compact);
                let surface = visual.debug_bounds("composer-surface").unwrap();
                let send = visual.debug_bounds("live-chat-send").unwrap();
                assert!(send.left() >= surface.left() && send.right() <= surface.right());
                if compact {
                    tap(visual, "composer-settings");
                    assert!(visual.debug_bounds("model-controls").is_some());
                    assert!(
                        visual
                            .debug_bounds("plugin-control-statistics-context")
                            .is_none()
                    );
                    assert!(visual.debug_bounds("composer-mode-page").is_none());
                    tap(visual, "composer-permission-page");
                    assert!(
                        visual
                            .debug_bounds("composer_permission_ask-option")
                            .is_some()
                    );
                    assert!(
                        visual
                            .debug_bounds("composer_permission_full-option")
                            .is_some()
                    );
                    assert!(
                        visual
                            .debug_bounds("composer_permission_project-option")
                            .is_none()
                    );
                    visual.simulate_keystrokes("escape");
                    wait(visual, |_| true);
                }
                view.read_with(visual, |view, cx| {
                    assert_eq!(
                        view.input.read(cx).value(),
                        "Keep this connection draft 中文"
                    );
                    assert_eq!(view.assistant.as_ref(), Some(&assistant));
                    assert_eq!(view.resource, Some(resource));
                    assert_eq!(view.binding.client.target(), fixture.node.id());
                    assert!(view.session().is_none());
                    assert_eq!(
                        view.composer_options.permissions,
                        &[Permission::Ask, Permission::Full]
                    );
                    assert!(!view.composer_options.skills);
                    assert!(matches!(
                        (resource, view.composer_options.mentions),
                        (Resource::Database(_), Mentions::Database)
                            | (Resource::Ssh(_), Mentions::Attachments)
                    ));
                });
            }
            visual.update(|window, _| window.remove_window());
            drop(view);
        }
        let Output::Snapshot(after) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected");
        };
        assert_eq!(
            after, before,
            "rendering connection drafts must not create sessions or run tasks"
        );
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        fixture.close();
    }
}
