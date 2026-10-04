use super::*;

#[gpui::test]
fn exchanges_turns(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        cx.update(|cx| {
            Theme::change(
                if remote {
                    ThemeMode::Light
                } else {
                    ThemeMode::Dark
                },
                None,
                cx,
            )
        });
        let fixture = Fixture::new(remote);
        fixture.game_plugin_with("reversi", &["ai_model"], |root| {
            board_games::anchors(root, "reversi")
        });
        let server = fixture
            .runtime
            .block_on(crate::agent_fixture::Server::markdown_after(
                "{\"move\":0}".into(),
                Duration::from_millis(1600),
            ));
        fixture.game_endpoint(&server.endpoint);
        let (panel, visual) = mount(&fixture, cx);
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
        open_package(visual, &panel, "reversi");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("reversi-status-ready")
        });
        assert!(server.requests.lock().unwrap().is_empty());

        click(visual, "reversi-start-black");
        wait(visual, |cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("reversi-status-yourTurn")
                && tree.contains("reversi-board")
                && tree.contains("reversi-legal-2-3")
                && tree.matches("reversi-disc-").count() == 4
        });
        for (width, height) in [(960., 680.), (1280., 820.)] {
            visual.simulate_window_resize(handle, size(px(width), px(height)));
            board_games::layout(visual, "reversi");
        }
        click(visual, "reversi-cell-2-3");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("reversi-status-thinking")
        });
        assert!(visual.debug_bounds("reversi-status-avatar").is_some());
        wait(visual, |cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("reversi-status-yourTurn")
                && tree.contains("reversi-disc-2-3")
                && tree.matches("reversi-disc-").count() == 6
        });
        let tree = visual.update(|_, cx| snapshot(&panel, cx));
        assert!(tree.contains("reversi-history-avatar-2"));
        assert!(tree.find("reversi-move-2").unwrap() < tree.find("reversi-move-1").unwrap());
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let prompt = requests[0]["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap();
        let choice: serde_json::Value =
            serde_json::from_str(prompt.lines().last().unwrap()).unwrap();
        assert_eq!(choice["state"]["turn"], "O");
        assert_eq!(choice["state"]["counts"]["black"], 4);
        assert_eq!(choice["state"]["counts"]["white"], 1);
        assert_eq!(
            choice["state"]["board"][2].as_str().unwrap().chars().nth(3),
            Some('X')
        );
        drop(requests);

        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}
