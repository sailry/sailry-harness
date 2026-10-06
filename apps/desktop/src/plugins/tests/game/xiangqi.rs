use super::*;

fn layout(visual: &mut VisualTestContext) {
    assert_player_strip(visual, &["xiangqi-avatar-red", "xiangqi-avatar-black"]);
    let board = visual.debug_bounds("xiangqi-board-area").unwrap();
    let status = visual.debug_bounds("xiangqi-status-area").unwrap();
    let history = visual.debug_bounds("xiangqi-history-area").unwrap();
    assert!(board.size.width >= px(350.) && board.size.height >= px(390.));
    assert!(visual.debug_bounds("xiangqi-avatar-red").unwrap().bottom() < board.top());
    assert!(board.right() < history.left());
    assert!(status.top() > board.bottom());
    assert!(status.bottom() < visual.debug_bounds("plugin-panel").unwrap().bottom());
}

#[gpui::test]
fn exchanges_turns(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
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
        let (_, server) = fixture.game_plugin_with("xiangqi", &["ai_model"], |root| {
            board_games::anchors(root, "xiangqi");
        });
        let (panel, visual) = mount(&fixture, cx);
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
        open_package(visual, &panel, "xiangqi");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("xiangqi-status-ready")
        });
        assert!(server.requests.lock().unwrap().is_empty());

        click(visual, "xiangqi-start-red");
        wait(visual, |cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("xiangqi-status-yourTurn")
                && tree.contains("xiangqi-board")
                && tree.contains("xiangqi-piece-6-0")
        });
        layout(visual);
        click(visual, "xiangqi-cell-6-0");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("xiangqi-legal-5-0")
        });
        click(visual, "xiangqi-cell-5-0");
        wait(visual, |cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("xiangqi-status-yourTurn")
                && tree.contains("xiangqi-piece-5-0")
                && tree.contains("xiangqi-move-2")
        });
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let prompt = requests[0]["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap();
        let choice: serde_json::Value =
            serde_json::from_str(prompt.lines().last().unwrap()).unwrap();
        assert_eq!(choice["state"]["turn"], "black");
        assert_eq!(choice["state"]["last"], "A4-A5");
        assert!(choice["state"]["fen"].as_str().unwrap().contains(" b"));
        assert!(!choice["choices"].as_array().unwrap().is_empty());
        drop(requests);

        visual.simulate_window_resize(handle, size(px(960.), px(680.)));
        layout(visual);
        visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
        layout(visual);
        click(visual, "xiangqi-new-game");
        wait(visual, |cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("xiangqi-status-yourTurn") && !tree.contains("xiangqi-move-1")
        });
        assert_eq!(server.requests.lock().unwrap().len(), 1);

        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}
