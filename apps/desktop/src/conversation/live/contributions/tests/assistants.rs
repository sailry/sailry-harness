use super::*;
use crate::conversation::live::tests::fixture::Harness;
use sailry_protocol::{DatabaseId, connection::Resource, database};

fn open_assistant(
    cx: &mut TestAppContext,
    mut binding: Binding,
    session: Option<Session>,
    assistant: plugin::conversation::Binding,
    resource: Option<Resource>,
) -> (Entity<View>, &mut VisualTestContext) {
    binding.project = None;
    binding.worktree = None;
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view =
            cx.new(|cx| View::for_assistant(binding, session, assistant, resource, window, cx));
        entity = Some(view.clone());
        let harness = cx.new(|_| Harness(view));
        Root::new(harness, window, cx)
    });
    (entity.unwrap(), visual)
}

#[gpui::test]
fn restricts_drafts_to_the_assistant_package(cx: &mut TestAppContext) {
    init(cx);
    cx.update(crate::plugins::init);
    for remote in [false, true] {
        let mut fixture = Fixture::with_tools(remote, vec![]);
        let notes = install(&mut fixture);
        let Output::Plugin(databases) = fixture.execute(Command::ReadPlugin {
            name: "databases".into(),
        }) else {
            panic!("Database package expected")
        };
        let path = fixture.directory.path().join("assistant.sqlite3");
        drop(rusqlite::Connection::open(&path).unwrap());
        let profile = database::Profile {
            sharing: None,
            id: DatabaseId::new(),
            revision: 0,
            name: "Assistant scope fixture".into(),
            connection: database::Connection::Sqlite {
                path: path.to_str().unwrap().into(),
            },
            read_only: true,
        };
        let Output::DatabaseProfile(profile) = fixture.execute(Command::SaveDatabase {
            profile,
            expected_revision: 0,
            password: None,
        }) else {
            panic!("Database profile expected")
        };
        let assistant = plugin::conversation::Binding {
            package: databases.summary.reference(),
            id: "database".into(),
        };
        let resource = Resource::Database(profile.id);
        let mut config = fixture.session.config.clone();
        config.assistant = Some(assistant.clone());
        config.resource = Some(resource);
        let Output::Session(session) = fixture.execute(Command::CreateSession {
            project: None,
            worktree: None,
            config: Some(config),
        }) else {
            panic!("Assistant session expected")
        };
        for session in [None, Some(session)] {
            let draft = session.is_none();
            let (view, visual) = open_assistant(
                cx,
                fixture.binding.clone(),
                session,
                assistant.clone(),
                Some(resource),
            );
            wait(visual, |cx| {
                view.read(cx).connected() && view.read(cx).contributions.read(cx).ready(cx)
            });
            assert_eq!(
                view.read_with(visual, |view, _| view.session().is_none()),
                draft
            );
            view.read_with(visual, |view, cx| {
                for name in ["task-notes", "worktrees", "git", "goals"] {
                    assert!(
                        view.plugin_panel(name, cx).is_none(),
                        "unrelated contribution controller mounted: {name}"
                    );
                }
                assert!(
                    view.plugin_panel("statistics", cx).is_some(),
                    "shared statistics controller missing"
                );
            });
            assert!(visual.debug_bounds("plugin-control-goals-goal").is_none());
            assert!(visual.debug_bounds("composer-settings").is_none());
            visual.update(|window, _| window.remove_window());
            drop(view);
        }

        let (view, visual) = open_assistant(
            cx,
            fixture.binding.clone(),
            None,
            plugin::conversation::Binding {
                package: notes.summary.reference(),
                id: "notes".into(),
            },
            None,
        );
        wait(visual, |cx| value(&view, "notes", cx) == Some(json!(2)));
        assert!(view.read_with(visual, |view, cx| {
            view.plugin_panel("task-notes", cx).is_some()
        }));
        assert!(view.read_with(visual, |view, cx| {
            view.plugin_panel("worktrees", cx).is_none()
        }));
        assert!(visual.debug_bounds("composer-settings").is_none());
        for selector in [
            "plugin-control-task-notes-refresh",
            "plugin-control-task-notes-actions",
            "plugin-control-task-notes-note",
        ] {
            assert!(
                visual.debug_bounds(selector).is_some(),
                "wide assistant action missing: {selector}"
            );
        }
        visual.update(|window, _| window.remove_window());
        drop(view);

        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| value(&view, "notes", cx) == Some(json!(2)));
        assert!(view.read_with(visual, |view, cx| {
            view.plugin_panel("task-notes", cx).is_some()
        }));
        resize(visual, 520.);
        assert!(visual.debug_bounds("composer-settings").is_some());
        visual.update(|window, _| window.remove_window());
        drop(view);
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        assert_eq!(
            snapshot.sessions.len(),
            2,
            "draft mounting must not create sessions"
        );
        assert_eq!(fixture.task_requests(), 0);
        fixture.close();
    }
}
