use super::*;

#[gpui::test]
fn exchanges_turns(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (_, server) = fixture.game_plugin("xiangqi", &["ai_model"]);
        let (panel, visual) = mount(&fixture, cx);
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
        open_package(visual, &panel, "xiangqi");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("xiangqi-status-ready")
        });
        assert!(server.requests.lock().unwrap().is_empty());

        click_game(visual, 580., 460.);
        wait(visual, |cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("xiangqi-status-yourTurn")
                && tree.contains("xiangqi-board")
                && tree.contains("xiangqi-piece-6-0")
        });
        click_game(visual, 338., 460.);
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("xiangqi-legal-5-0")
        });
        click_game(visual, 338., 414.);
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

        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}
