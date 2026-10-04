use super::*;

#[gpui::test]
fn challenges_a_bid(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (_, server) = fixture.game_plugin("liars-dice", &["player_1_model", "player_2_model"]);
        let (panel, visual) = mount(&fixture, cx);
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
        open_package(visual, &panel, "liars-dice");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("liars-status-ready")
        });
        assert!(server.requests.lock().unwrap().is_empty());

        click_game(visual, 640., 510.);
        wait(visual, |cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("liars-table")
                && tree.contains("liars-status-yourTurn")
                && tree.contains("liars-hand-0")
                && tree.contains("liars-action-bid")
        });
        click_game(visual, 640., 545.);
        wait(visual, |cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("liars-round-result") && tree.contains("liars-action-next")
        });
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let prompt = requests[0]["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap();
        let choice: serde_json::Value =
            serde_json::from_str(prompt.lines().last().unwrap()).unwrap();
        assert_eq!(choice["state"]["player"], 1);
        assert_eq!(choice["state"]["hand"].as_array().unwrap().len(), 5);
        assert_eq!(choice["state"]["counts"], serde_json::json!([5, 5, 5]));
        assert_eq!(choice["state"]["currentBid"]["quantity"], 1);
        assert_eq!(choice["state"]["currentBid"]["face"], 1);
        assert_eq!(choice["choices"][0]["kind"], "challenge");
        assert!(choice["state"]["hands"].is_null());
        drop(requests);

        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}
