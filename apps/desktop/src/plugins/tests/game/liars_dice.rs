use super::*;

fn anchors(root: &std::path::Path) {
    for file in ["controls.js", "table.js", "view.js"] {
        let path = root.join("dev.sailry.platform/desktop").join(file);
        let source = std::fs::read_to_string(&path).unwrap();
        let source = match file {
            "controls.js" => source.replace(
                ".label(label)",
                ".child(TestAnchor.new(id).child(div().child(label)))",
            ),
            "table.js" => source
                .replace(
                    "return div().id(\"liars-felt\")",
                    "return TestAnchor.new(\"liars-board-area\").child(div().id(\"liars-felt\")",
                )
                .replace(
                    ".child(center).child(actions);",
                    ".child(center).child(actions));",
                ),
            _ => source
                .replace(
                    ".child(hand(view, 0, compact))",
                    ".child(TestAnchor.new(\"liars-hand-area\").child(hand(view, 0, compact)))",
                )
                .replace(
                    ".child(history(view))",
                    ".child(TestAnchor.new(\"liars-history-area\").child(history(view)))",
                ),
        };
        std::fs::write(
            path,
            format!("import {{ Anchor as TestAnchor }} from \"sailry/test\";\n{source}"),
        )
        .unwrap();
    }
}

fn layout(visual: &mut VisualTestContext) {
    assert_player_strip(
        visual,
        &["liars-avatar-1", "liars-avatar-0", "liars-avatar-2"],
    );
    let board = visual.debug_bounds("liars-board-area").unwrap();
    let hand = visual.debug_bounds("liars-hand-area").unwrap();
    let history = visual.debug_bounds("liars-history-area").unwrap();
    assert!(visual.debug_bounds("liars-avatar-0").unwrap().bottom() < board.top());
    assert!(board.right() <= history.left());
    assert!(hand.top() > board.bottom());
    assert!(hand.bottom() < visual.debug_bounds("plugin-panel").unwrap().bottom());
}

#[gpui::test]
fn challenges_a_bid(cx: &mut TestAppContext) {
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
        let (_, server) =
            fixture.game_plugin_with("liars-dice", &["player_1_model", "player_2_model"], anchors);
        let (panel, visual) = mount(&fixture, cx);
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
        open_package(visual, &panel, "liars-dice");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("liars-status-ready")
        });
        assert!(server.requests.lock().unwrap().is_empty());

        click(visual, "liars-start");
        wait(visual, |cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("liars-table")
                && tree.contains("liars-status-yourTurn")
                && tree.contains("liars-hand-0")
                && tree.contains("liars-action-bid")
        });
        layout(visual);
        click(visual, "liars-bid");
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

        visual.simulate_window_resize(handle, size(px(960.), px(680.)));
        layout(visual);
        click(visual, "liars-next");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("liars-status-yourTurn")
                || server.requests.lock().unwrap().len() > 1
        });
        layout(visual);
        visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
        layout(visual);

        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}
