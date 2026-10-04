use super::*;

fn id(game: &str, suffix: &str) -> &'static str {
    Box::leak(format!("{game}-{suffix}").into_boxed_str())
}

pub(super) fn anchors(root: &std::path::Path, game: &str) {
    let path = root.join("dev.sailry.platform/desktop/controls.js");
    let source = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        path,
        format!(
            "import {{ Anchor as TestAnchor }} from \"sailry/test\";\n{}",
            source.replace(
                ".label(label)",
                ".child(TestAnchor.new(id).child(div().child(label)))"
            )
        ),
    )
    .unwrap();
    let path = root.join("dev.sailry.platform/desktop/board.js");
    let source = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        path,
        format!(
            "import {{ Anchor as TestAnchor }} from \"sailry/test\";\n{}",
            source.replace(
                ".child(content);",
                &format!(
                    ".child(TestAnchor.new(`{game}-cell-${{row}}-${{column}}`).child(content));"
                )
            )
        ),
    )
    .unwrap();
    let path = root.join("dev.sailry.platform/desktop/view.js");
    let source = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        path,
        format!(
            "import {{ Anchor as TestAnchor }} from \"sailry/test\";\n{}",
            source
                .replace(
                    ".child(board(view))",
                    &format!(".child(TestAnchor.new(\"{game}-board-area\").child(board(view)))")
                )
                .replace(
                    ".child(status(view))",
                    &format!(".child(TestAnchor.new(\"{game}-status-area\").child(status(view)))")
                )
                .replace(
                    ".child(history(view))",
                    &format!(
                        ".child(TestAnchor.new(\"{game}-history-area\").child(history(view)))"
                    )
                )
        ),
    )
    .unwrap();
}

pub(super) fn layout(visual: &mut VisualTestContext, game: &str) {
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert_player_strip(visual, &[id(game, "avatar-1"), id(game, "avatar-2")]);
    let board = visual.debug_bounds(id(game, "board-area")).unwrap();
    let status = visual.debug_bounds(id(game, "status-area")).unwrap();
    let history = visual.debug_bounds(id(game, "history-area")).unwrap();
    assert!(board.size.width >= px(350.) && board.size.height >= px(350.));
    assert!(visual.debug_bounds(id(game, "avatar-1")).unwrap().bottom() < board.top());
    assert!(board.right() < history.left());
    assert!(status.top() > board.bottom());
    let panel = visual.debug_bounds("plugin-panel").unwrap();
    assert!(status.bottom() < panel.bottom());
}

#[gpui::test]
fn results_preserve_the_board_and_start_another_game(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        for game in ["gomoku", "reversi"] {
            for (winner, outcome) in [(2, "win"), (1, "lose"), (0, "draw")] {
                let fixture = Fixture::new(remote);
                let (_, _server) = fixture.game_plugin_with(game, &["ai_model"], |root| {
                    anchors(root, game);
                    let path = root.join("dev.sailry.platform/desktop/main.js");
                    let source = std::fs::read_to_string(&path).unwrap();
                    let position = if game == "reversi" {
                        format!("this.game.board = Array.from({{length:8}}, (_, row) => Array(8).fill(row < {} ? 1 : 2));", if winner == 1 { 5 } else if winner == 2 { 3 } else { 4 })
                    } else { String::new() };
                    std::fs::write(path, source.replace("this.game = newGame(human);", &format!(
                        "this.game = newGame(human); if (!this.fixtureFinished) {{ this.fixtureFinished = true; {position} this.game.phase = 'over'; this.game.winner = {winner}; }}"
                    ))).unwrap();
                });
                let (panel, visual) = mount(&fixture, cx);
                let handle = visual.update(|window, _| window.window_handle());
                visual.simulate_window_resize(handle, size(px(960.), px(680.)));
                open_package(visual, &panel, game);
                wait(visual, |cx| {
                    snapshot(&panel, cx).contains(id(game, "status-ready"))
                });
                click(visual, id(game, "start-white"));
                wait(visual, |cx| {
                    snapshot(&panel, cx).contains(id(game, &format!("result-{outcome}")))
                });
                let tree = visual.update(|_, cx| snapshot(&panel, cx));
                assert!(tree.contains(id(
                    game,
                    &format!("winner-{}", if winner == 0 { 2 } else { winner })
                )));
                assert!(
                    visual
                        .debug_bounds(id(game, "play-again"))
                        .unwrap()
                        .bottom()
                        < px(680.)
                );
                if outcome == "lose" {
                    click(visual, id(game, "play-again"));
                } else {
                    if outcome == "draw" {
                        visual.simulate_keystrokes("escape");
                    } else {
                        click(visual, id(game, "view-board"));
                    }
                    wait(visual, |cx| {
                        !snapshot(&panel, cx).contains(id(game, &format!("result-{outcome}")))
                    });
                    assert!(
                        visual.update(|_, cx| snapshot(&panel, cx).contains(id(game, "board")))
                    );
                    click(visual, id(game, "new-game"));
                }
                wait(visual, |cx| {
                    snapshot(&panel, cx).contains(id(game, "status-yourTurn"))
                });
                let tree = visual.update(|_, cx| snapshot(&panel, cx));
                assert!(
                    !tree.contains(id(game, "result-win"))
                        && !tree.contains(id(game, "result-lose"))
                        && !tree.contains(id(game, "result-draw"))
                );
                visual.update(|window, _| window.remove_window());
                drop(panel);
                fixture.close();
            }
        }
    }
}
