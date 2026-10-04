use super::*;
use sailry_protocol::{Permission, connection::Resource, database, ssh};

#[gpui::test]
fn scopes_and_full_permission(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let id = sailry_protocol::DatabaseId::new();
        let fixture = Fixture::with_tools(
            remote,
            vec![(
                crate::agent_fixture::plugin_tool("databases", "database_execute"),
                serde_json::json!({"connection":id,"sql":"INSERT INTO items VALUES (42)"}),
            )],
        );
        let path = fixture.directory.path().join("data.sqlite3");
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE items(value INTEGER)")
            .unwrap();
        fixture.execute(Command::SaveDatabase {
            profile: database::Profile {
                id,
                revision: 0,
                name: "Bound data".into(),
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
                name: "Bound host".into(),
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
        let mut entity = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| {
                View::for_connection(
                    fixture.binding.clone(),
                    None,
                    Resource::Database(id),
                    ComposerOptions::connection(Mentions::Database),
                    window,
                    cx,
                )
            });
            entity = Some(view.clone());
            Root::new(cx.new(|_| fixture::Harness(view)), window, cx)
        });
        let view = entity.unwrap();
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).config.is_some()
        });
        assert_eq!(
            view.read_with(visual, |view, _| view.config.as_ref().unwrap().permission),
            Permission::Ask
        );
        tap(visual, "live-chat-input");
        visual.simulate_input("@");
        wait(visual, |cx| {
            !view.read(cx).references.loading && view.read(cx).references.open
        });
        view.read_with(visual, |view, _| {
            assert_eq!(view.references.page, Page::Database(id, "Bound data".into(), None));
            assert!(matches!(&view.references.rows[0], Item::Current(_)));
            assert_eq!(view.references.rows[0].label(), tr("reference_select_current"));
            assert!(matches!(&view.references.rows[1], Item::Page(Page::Database(_, _, Some(name))) if name == "main"));
            assert!(matches!(view.references.rows.last(), Some(Item::Attachment)));
        });
        // Select the entire connection without traversing to a leaf table.
        tap(visual, "live-reference-row-0");
        visual.simulate_input("@");
        wait(visual, |cx| !view.read(cx).references.loading);
        tap(visual, "live-reference-row-1");
        wait(visual, |cx| !view.read(cx).references.loading);
        assert!(view.read_with(visual, |view, _| matches!(&view.references.rows[0], Item::Current(Reference { target: Target::Database { database: Some(name), table: None, .. }, .. }) if name == "main")));
        tap(visual, "live-reference-row-0");
        visual.simulate_input("@");
        wait(visual, |cx| !view.read(cx).references.loading);
        tap(visual, "live-reference-row-1");
        wait(visual, |cx| !view.read(cx).references.loading);
        tap(visual, "live-reference-row-1");
        view.read_with(visual, |view, cx| {
            assert_eq!(view.active_references(cx).len(), 3);
            assert!(view.references_valid(&view.active_references(cx)));
        });
        visual.simulate_input("/");
        wait(visual, |cx| view.read(cx).references.open);
        view.read_with(visual, |view, _| {
            assert!(!view.references.rows.iter().any(|item| matches!(
                item,
                Item::Command(commands::Choice::Prompt("review" | "test"))
                    | Item::Command(commands::Choice::Skill(..))
            )));
        });
        visual.simulate_keystrokes("escape");
        visual.simulate_keystrokes("cmd-a");
        visual.simulate_input("Insert a row");
        tap(visual, "live-chat-permission");
        assert!(
            visual
                .debug_bounds("composer_permission_readonly-option")
                .is_some()
        );
        assert!(
            visual
                .debug_bounds("composer_permission_project-option")
                .is_none()
        );
        tap(visual, "composer_permission_full-option");
        assert_eq!(
            view.read_with(visual, |view, _| view.config.as_ref().unwrap().permission),
            Permission::Full
        );
        tap(visual, "live-chat-send");
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    !snapshot.page.runs.is_empty()
                        && snapshot
                            .page
                            .runs
                            .iter()
                            .all(|run| run.status == Status::Completed)
                })
        });
        assert_eq!(
            db.query_row("SELECT value FROM items", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            42
        );
        view.read_with(visual, |view, _| {
            let approvals = &view.history.snapshot.as_ref().unwrap().page.approvals;
            assert_eq!(approvals.len(), 1);
            assert_eq!(
                approvals[0].source,
                sailry_protocol::conversation::ApprovalSource::Full
            );
        });
        let first = view.read_with(visual, |view, _| view.session().unwrap());
        visual
            .update(|window, cx| view.update(cx, |view, cx| view.switch_session(None, window, cx)));
        wait(visual, |cx| view.read(cx).can_switch_session());
        view.read_with(visual, |view, _| {
            assert!(view.session.is_none() && view.history.snapshot.is_none());
            assert_eq!(view.config.as_ref().unwrap().permission, Permission::Ask);
            assert_eq!(view.connection_sessions().len(), 1);
            assert_eq!(view.connection_sessions()[0].id, first);
            assert!(!view.connection_sessions()[0].archived);
        });
        visual.simulate_input("Insert another row");
        tap(visual, "live-chat-send");
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot.page.approvals.iter().any(|approval| {
                        approval.state == sailry_protocol::conversation::ApprovalState::Pending
                    })
                })
        });
        let approval = view.read_with(visual, |view, _| {
            view.history.snapshot.as_ref().unwrap().page.approvals[0].id
        });
        tap(visual, &format!("live-approval-approve-{approval}"));
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    !snapshot.page.runs.is_empty()
                        && snapshot
                            .page
                            .runs
                            .iter()
                            .all(|run| run.status == Status::Completed)
                })
        });
        view.read_with(visual, |view, _| {
            assert_ne!(view.session(), Some(first));
            assert_eq!(
                view.session.as_ref().unwrap().config.resource,
                Some(Resource::Database(id))
            );
        });
        wait(visual, |cx| view.read(cx).connection_sessions().len() == 2);
        view.read_with(visual, |view, _| {
            assert_eq!(
                view.connection_sessions()
                    .iter()
                    .map(|session| session.id)
                    .collect::<Vec<_>>(),
                [view.session().unwrap(), first]
            );
        });
        visual.update(|window, cx| {
            view.update(cx, |view, cx| view.switch_session(Some(first), window, cx))
        });
        wait(visual, |cx| view.read(cx).connected());
        assert_eq!(
            view.read_with(visual, |view, _| view.config.as_ref().unwrap().permission),
            Permission::Full
        );
        // Configuration changes preserve the Node's creation/manual order.
        let order = view.read_with(visual, |view, _| {
            view.connection_sessions()
                .iter()
                .map(|session| session.id)
                .collect::<Vec<_>>()
        });
        // Returning to read-only changes future turns on the same bound session.
        tap(visual, "live-chat-permission");
        tap(visual, "composer_permission_readonly-option");
        wait(visual, |cx| {
            view.read(cx).config.as_ref().unwrap().permission == Permission::Ask
                && !view.read(cx).busy()
        });
        assert_eq!(
            view.read_with(visual, |view, _| {
                view.connection_sessions()
                    .iter()
                    .map(|session| session.id)
                    .collect::<Vec<_>>()
            }),
            order
        );
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                *view = View::for_connection(
                    fixture.binding.clone(),
                    None,
                    Resource::Ssh(ssh),
                    ComposerOptions::connection(Mentions::Attachments),
                    window,
                    cx,
                );
                view.focus(window, cx);
            })
        });
        wait(visual, |cx| view.read(cx).connected());
        visual.simulate_input("@");
        wait(visual, |cx| view.read(cx).references.open);
        view.read_with(visual, |view, _| {
            assert!(view.connection_sessions().is_empty());
            assert_eq!(view.references.rows.len(), 1);
            assert!(matches!(view.references.rows[0], Item::Attachment));
            assert_eq!(view.config.as_ref().unwrap().permission, Permission::Ask);
        });
        tap(visual, "live-reference-row-0");
        assert!(visual.did_prompt_for_paths());
        visual.simulate_path_prompt_response(|_| None);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
