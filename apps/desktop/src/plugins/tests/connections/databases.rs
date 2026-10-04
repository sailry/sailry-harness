use super::*;

#[gpui::test]
fn creates_queries_and_observes_the_captured_assistant(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = install(&fixture, "databases");
        let path = fixture.directory.path().join("rows.sqlite3");
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE items(id INTEGER PRIMARY KEY,value TEXT); INSERT INTO items VALUES(9223372036854775807,'Exact 中文 🙂')").unwrap();
        let (shell, panel, visual) = mount(&fixture, remote, "databases", cx);
        landing_width(visual, "db");
        tap(visual, "db-new");
        shown(&panel, visual, "db-editor");
        input(visual, "field-2", "Captured database");
        input(visual, "field-3", path.to_str().unwrap());
        tap(visual, "db-test-edit");
        toast(visual, "Connected");
        assert!(panel.read_with(visual, |_, cx| {
            let view = snapshot(&panel, cx);
            view.contains("db-editor") && !view.contains("db-status")
        }));
        tap(visual, "field-2");
        visual.simulate_keystrokes("secondary-a secondary-c");
        assert_eq!(
            visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            "Captured database"
        );
        assert!(state(&fixture).databases.is_empty());
        tap(visual, "db-save");
        wait(visual, |_| state(&fixture).databases.len() == 1);
        let profile = state(&fixture).databases[0].clone();
        enabled(&panel, visual, &format!("db-open-{}", profile.id));
        landing_width(visual, "db");
        tap(visual, &format!("db-open-{}", profile.id));
        shown(&panel, visual, "db-page");
        shown(&panel, visual, "resource-file-db:0");
        assert_eq!(
            visual
                .debug_bounds("database-catalog-header")
                .unwrap()
                .size
                .height,
            gpui_kit::component::Size::Medium.table_row_height(),
        );
        assert!(visual.debug_bounds("resource-file-table:0:0").is_none());
        tap(visual, "resource-file-db:0");
        shown(&panel, visual, "items");
        shown(&panel, visual, "resource-file-table:0:0");
        no_session_navigation(visual);
        assert!(visual.debug_bounds("shell-navigation").is_some());
        assert!(visual.debug_bounds("header-sidebar-toggle").is_some());
        wait(visual, |cx| {
            panel
                .read(cx)
                .mounted
                .as_ref()
                .unwrap()
                .conversations
                .first()
                .is_some_and(|chat| chat.read(cx).connected())
        });
        for selector in ["connection-chat-new", "connection-chat-history"] {
            assert!(
                visual.debug_bounds(selector).unwrap().top() < px(crate::preview::HEADER_HEIGHT),
                "assistant controls must stay in the details header: {selector}",
            );
        }
        assert_eq!(
            state(&fixture).sessions.len(),
            1,
            "mounting does not create a session"
        );
        tap(visual, "db-sql-tab");
        input(visual, "field-1", "SELECT id,value FROM items");
        tap(visual, "db-run");
        shown(&panel, visual, "9223372036854775807");
        shown(&panel, visual, "Exact 中文 🙂");
        let query=fixture.transport.requests.lock().unwrap().iter().find(|request|matches!(&request.command,Command::QueryDatabase{sql,..} if sql=="SELECT id,value FROM items")).cloned().unwrap();
        assert!(
            query
                .plugin
                .as_ref()
                .is_some_and(|context| context.package == package.summary.reference()
                    && context.worktree.is_none()
                    && context.session.is_none())
        );
        let server = fixture
            .runtime
            .block_on(crate::agent_fixture::Server::tools(vec![(
                crate::agent_fixture::plugin_tool("databases", "database_query"),
                serde_json::json!({"connection":profile.id,"sql":"SELECT value,id FROM items"}),
            )]));
        let mut provider = state(&fixture).providers[0].clone();
        provider.endpoint = server.endpoint.clone();
        fixture.execute(Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        });
        tap(visual, "live-chat-input");
        visual.simulate_input("Inspect the saved data");
        visual.simulate_keystrokes("enter");
        wait(visual, |_| state(&fixture).sessions.len() == 2);
        tap(visual, "db-history-tab");
        shown(&panel, visual, "SELECT value,id FROM items");
        shown(&panel, visual, "Exact 中文 🙂");
        let row_selector: &'static str =
            Box::leak(format!("db-history-{}", query.id).into_boxed_str());
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let row_height = gpui_kit::component::Size::Medium.table_row_height();
        assert_eq!(
            visual.debug_bounds(row_selector).unwrap().size.height,
            row_height
        );
        let history_height = visual
            .debug_bounds("database-history-grid")
            .unwrap()
            .size
            .height;
        let results = visual.debug_bounds("database-results").unwrap();
        let divider = point(results.center().x, results.bottom());
        visual.simulate_mouse_move(divider, None, Modifiers::default());
        visual.simulate_mouse_down(divider, MouseButton::Left, Modifiers::default());
        for step in 1..=4 {
            visual.simulate_mouse_move(
                divider - point(px(0.), px(25. * step as f32)),
                MouseButton::Left,
                Modifiers::default(),
            );
            wait(visual, |_| true);
        }
        visual.simulate_mouse_up(
            divider - point(px(0.), px(100.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            visual.executor().advance_clock(Duration::from_millis(10));
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            if visual
                .debug_bounds("database-history-grid")
                .is_some_and(|bounds| bounds.size.height > history_height + px(50.))
            {
                break;
            }
            assert!(Instant::now() < deadline, "history resize deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            visual.debug_bounds(row_selector).unwrap().size.height,
            row_height
        );
        assert_eq!(
            (visual.debug_bounds("db-log-sql-1").unwrap().top()
                - visual.debug_bounds("db-log-sql-0").unwrap().top())
            .abs(),
            row_height
        );
        let snapshot = state(&fixture);
        let session = snapshot
            .sessions
            .iter()
            .find(|session| {
                session.config.resource
                    == Some(sailry_protocol::connection::Resource::Database(profile.id))
            })
            .unwrap();
        assert!(session.project.is_none());
        assert_eq!(
            session.config.assistant.as_ref().unwrap().package,
            package.summary.reference()
        );
        wait(
            visual,
            |_| matches!(fixture.execute(Command::ReadConversation {session:session.id,before:None,limit:100}), Output::Conversation(history) if history.page.entries.iter().flat_map(|entry|&entry.parts).any(|part|matches!(part,sailry_protocol::conversation::Part::ToolResult{result,..} if result["data"]["data"]["rows"][0][1]["value"].as_i64()==Some(i64::MAX)))),
        );
        let Output::Conversation(history) = fixture.execute(Command::ReadConversation {
            session: session.id,
            before: None,
            limit: 100,
        }) else {
            panic!("history expected");
        };
        assert!(history.page.entries.iter().flat_map(|entry|&entry.parts).any(|part|matches!(part,sailry_protocol::conversation::Part::ToolResult{result,..} if result["data"]["data"]["rows"][0][1]["value"].as_i64()==Some(i64::MAX))));
        let writes=fixture.transport.requests.lock().unwrap().iter().filter(|request|matches!(&request.command,Command::QueryDatabase{sql,..} if sql=="SELECT id,value FROM items")).count();
        assert_eq!(writes, 1);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.close();
    }
}
