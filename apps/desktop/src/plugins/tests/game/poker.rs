use super::*;

fn anchors(root: &std::path::Path) {
    let path = root.join("dev.sailry.platform/desktop/controls.js");
    let source = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        path,
        format!(
            "import {{ Anchor as TestAnchor }} from \"sailry/test\";\n{}",
            source.replace(
                ".label(label)",
                ".child(TestAnchor.new(id).child(div().child(label)))"
            ).replace(
                "new ShimmerText(text.pondering).id(\"poker-thinking\").text_sm()",
                "TestAnchor.new(\"poker-thinking-text\").child(new ShimmerText(text.pondering).id(\"poker-thinking\").text_sm())"
            ).replace(
                ".child(`${view.elapsed} ${text.seconds}`)",
                ".child(TestAnchor.new(`poker-elapsed-${view.elapsed}-anchor`).child(div().child(`${view.elapsed} ${text.seconds}`)))"
            )
        ),
    )
    .unwrap();
    let path = root.join("dev.sailry.platform/desktop/table.js");
    let source = std::fs::read_to_string(&path).unwrap();
    std::fs::write(path, format!("import {{ Anchor as TestAnchor }} from \"sailry/test\";\n{}", source
        .replace(".child(open ? card(game.board[index], compact) : empty(compact))", ".child(TestAnchor.new(`poker-board-${index}-anchor`).child(open ? card(game.board[index], compact) : empty(compact)))")
        .replace(".child(cards(game.hands[player], 2, compact, player === 1 && !reveal))", ".child(TestAnchor.new(`poker-hole-${player}-anchor`).child(cards(game.hands[player], 2, compact, player === 1 && !reveal)))")
    )).unwrap();
}

#[gpui::test]
fn plays_a_human_and_model_betting_round(cx: &mut TestAppContext) {
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
        let (_, _server) = fixture.game_plugin_with("poker", &["player_1_model"], anchors);
        let release = sailry_link::CancellationToken::new();
        let delayed = fixture
            .runtime
            .block_on(crate::agent_fixture::Server::markdown_held(
                "{\"move\":0}".into(),
                release.clone(),
            ));
        fixture.game_endpoint(&delayed.endpoint);
        let (panel, visual) = mount(&fixture, cx);
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
        open_package(visual, &panel, "poker");
        wait(visual, |cx| {
            let state = panel.read(cx);
            state.mounted.is_some() || state.error.is_some()
        });
        assert!(
            panel.read_with(visual, |panel, _| panel.error.is_none()),
            "poker panel: {:?}",
            panel.read_with(visual, |panel, _| panel.error)
        );
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("poker-status-ready")
        });
        assert!(delayed.requests.lock().unwrap().is_empty());

        click(visual, "poker-start");
        wait(visual, |cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("poker-table") && tree.contains("poker-action-call")
        });
        assert!(visual.update(|_, cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("poker-seat-0")
                && tree.contains("poker-seat-1")
                && tree.contains("poker-community")
                && tree.contains("poker-pot")
                && tree.contains("poker-active-0")
                && tree.contains("poker-history")
                && !tree.contains("poker-status-avatar")
                && !tree.contains("poker-history-action-")
                && !tree.contains("poker-raise-input")
                && tree.contains("character-01.png")
                && tree.contains("character-02.png")
        }));
        for (width, height) in [(960., 680.), (960., 820.), (1280., 820.)] {
            visual.simulate_window_resize(handle, size(px(width), px(height)));
            assert_player_strip(visual, &["poker-avatar-0", "poker-avatar-1"]);
            let call = visual.debug_bounds("poker-call").unwrap();
            assert!(call.bottom() < px(height) && call.right() < px(width));
            assert!(visual.debug_bounds("poker-back-0").is_some());
            let hand = visual.debug_bounds("poker-hole-0-anchor").unwrap();
            let opponent = visual.debug_bounds("poker-hole-1-anchor").unwrap();
            let first = visual.debug_bounds("poker-board-0-anchor").unwrap();
            let last = visual.debug_bounds("poker-board-4-anchor").unwrap();
            assert_eq!(hand.size.height, px(if height < 760. { 64. } else { 92. }));
            assert_eq!(opponent.size.height, px(64.));
            for avatar in ["poker-avatar-0", "poker-avatar-1"] {
                assert!(visual.debug_bounds(avatar).unwrap().bottom() < first.top());
            }
            assert!(opponent.bottom() < first.top());
            assert!(((first.left() + last.right()) / 2. - hand.center().x).abs() <= px(1.));
            assert!(call.top() > last.bottom() && call.bottom() < hand.top());
            click(visual, "poker-open-raise");
            let confirm = visual.debug_bounds("poker-raise").unwrap();
            assert!(confirm.top() > px(0.) && confirm.bottom() < px(height));
            assert!(confirm.left() > px(0.) && confirm.right() < px(width));
            assert_eq!(visual.debug_bounds("poker-board-4-anchor").unwrap(), last);
            let tree = visual.update(|_, cx| snapshot(&panel, cx));
            assert!(tree.contains("poker-raise-dialog"));
            assert!(!tree.contains("poker-raise-input"));
            assert!(visual.debug_bounds("poker-min").is_some());
            let first = visual.debug_bounds("poker-raise-2x").unwrap();
            let second = visual.debug_bounds("poker-raise-3x").unwrap();
            let third = visual.debug_bounds("poker-raise-5x").unwrap();
            let fourth = visual.debug_bounds("poker-raise-10x").unwrap();
            assert_eq!(first.top(), second.top());
            assert_eq!(third.top(), fourth.top());
            assert!(first.right() < second.left() && third.top() > first.bottom());
            click(visual, "poker-cancel-raise");
        }
        click(visual, "poker-open-raise");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("poker-raise-dialog")
        });
        click(visual, "poker-raise-3x");
        let tree = visual.update(|_, cx| snapshot(&panel, cx));
        assert!(tree.contains("30"));
        assert!(delayed.requests.lock().unwrap().is_empty());
        visual.simulate_keystrokes("escape");
        wait(visual, |cx| {
            !snapshot(&panel, cx).contains("poker-raise-dialog")
        });
        click(visual, "poker-call");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("poker-status-thinking")
        });
        let tree = visual.update(|_, cx| snapshot(&panel, cx));
        assert!(!tree.contains("poker-controls"));
        assert!(!tree.contains("Acting") && !tree.contains("行动中"));
        assert!(tree.contains("poker-thinking"));
        assert!(tree.contains("poker-elapsed-0-anchor"));
        for (width, height) in [(960., 680.), (1280., 820.)] {
            visual.simulate_window_resize(handle, size(px(width), px(height)));
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let avatar = visual.debug_bounds("poker-status-avatar").unwrap();
            let label = visual.debug_bounds("poker-thinking-text").unwrap();
            let elapsed = visual.debug_bounds("poker-elapsed-0-anchor").unwrap();
            assert!(label.size.width > px(0.) && label.size.height > px(0.));
            assert!(avatar.right() <= label.left() && label.right() <= elapsed.left());
            assert!(elapsed.right() < px(width) && elapsed.bottom() < px(height));
        }
        wait(visual, |cx| {
            let tree = snapshot(&panel, cx);
            tree.split("poker-elapsed-").any(|suffix| {
                suffix
                    .split('-')
                    .next()
                    .and_then(|seconds| seconds.parse::<u64>().ok())
                    .is_some_and(|seconds| seconds >= 1)
            })
        });
        assert!(tree.contains("poker-history-action-0"));
        assert!(tree.contains("poker-history-avatar-0"));
        let thinking = visual.debug_bounds("poker-status-avatar").unwrap();
        assert!(
            thinking.top()
                > visual
                    .debug_bounds("poker-board-4-anchor")
                    .unwrap()
                    .bottom()
        );
        assert!(thinking.bottom() < visual.debug_bounds("poker-hole-0-anchor").unwrap().top());

        release.cancel();

        wait(visual, |cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("poker-action-check")
                && !tree.contains("poker-error")
                && delayed.requests.lock().unwrap().len() >= 2
        });
        let requests = delayed.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        for (index, request) in requests.iter().enumerate() {
            let prompt = request["messages"].as_array().unwrap().last().unwrap()["content"]
                .as_str()
                .unwrap();
            let choice: serde_json::Value =
                serde_json::from_str(prompt.lines().last().unwrap()).unwrap();
            assert_eq!(choice["state"]["player"], 1);
            assert_eq!(choice["state"]["hand"].as_array().unwrap().len(), 2);
            assert!(choice["state"]["hands"].is_null());
            assert_eq!(
                choice["state"]["stage"],
                if index == 0 { "preflop" } else { "flop" }
            );
        }
        drop(requests);

        click(visual, "poker-open-raise");
        click(visual, "poker-all-in");
        assert_eq!(delayed.requests.lock().unwrap().len(), 2);
        assert!(
            !visual
                .update(|_, cx| snapshot(&panel, cx))
                .contains("poker-result-dialog")
        );
        click(visual, "poker-raise");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("poker-result-dialog")
        });
        assert!(
            !visual
                .update(|_, cx| snapshot(&panel, cx))
                .contains("poker-controls")
        );
        assert!(
            visual.debug_bounds("poker-new-match").is_some()
                || visual.debug_bounds("poker-next-hand").is_some()
        );

        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn presents_results_and_resumes_after_dismissal(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        for (outcome, hands, board, finish, match_over) in [
            ("won", "[[48,49],[0,1]]", "[8,17,26,35,44]", "round", false),
            ("lost", "[[0,1],[48,49]]", "[8,17,26,35,44]", "round", false),
            ("split", "[[0,1],[4,5]]", "[32,36,40,44,48]", "round", false),
            ("lost", "[[0,1],[48,49]]", "[8,17,26,35,44]", "fold", false),
            ("won", "[[48,49],[0,1]]", "[8,17,26,35,44]", "all-in", true),
        ] {
            let fixture = Fixture::new(remote);
            let (_, _server) = fixture.game_plugin_with("poker", &["player_1_model"], |root| {
                anchors(root);
                let path = root.join("dev.sailry.platform/desktop/main.js");
                let source = std::fs::read_to_string(&path).unwrap();
                let finish = match finish {
                    "fold" => "act(this.game, 0, {kind: 'fold'});",
                    "all-in" => "act(this.game, 0, {kind: 'raise', to: 1000}); act(this.game, 1, {kind: 'call'});",
                    _ => "while (this.game.phase === 'betting') act(this.game, this.game.turn, {kind: legal(this.game).toCall ? 'call' : 'check'});",
                };
                std::fs::write(path, source.replacen("this.game = deal();", &format!(
                    "this.game = deal(); this.game.hands = {hands}; this.game.board = {board}; {finish}"
                ), 1)).unwrap();
            });
            let (panel, visual) = mount(&fixture, cx);
            let handle = visual.update(|window, _| window.window_handle());
            visual.simulate_window_resize(handle, size(px(960.), px(680.)));
            open_package(visual, &panel, "poker");
            wait(visual, |cx| {
                snapshot(&panel, cx).contains("poker-status-ready")
            });
            click(visual, "poker-start");
            wait(visual, |cx| {
                snapshot(&panel, cx).contains("poker-result-dialog")
            });
            let tree = visual.update(|_, cx| snapshot(&panel, cx));
            assert!(tree.contains(&format!("poker-result-{outcome}")));
            assert_eq!(tree.contains("poker-result-hand-0"), finish != "fold");
            assert_eq!(tree.contains("poker-back-0"), finish == "fold");
            let next = if match_over {
                "poker-new-match"
            } else {
                "poker-next-hand"
            };
            assert!(visual.debug_bounds(next).unwrap().bottom() < px(680.));
            assert!(tree.contains("poker-result-net"));
            assert!(!tree.contains("This hand") && !tree.contains("本手盈亏"));
            assert!(!tree.contains("poker-show-result"));
            if outcome == "lost" && finish == "round" {
                click(visual, next);
            } else {
                if outcome == "split" {
                    visual.simulate_keystrokes("escape");
                } else {
                    click(visual, "poker-close-result");
                }
                wait(visual, |cx| {
                    !snapshot(&panel, cx).contains("poker-result-dialog")
                });
                let table_next = if match_over {
                    "poker-table-new-match"
                } else {
                    "poker-table-next-hand"
                };
                assert!(visual.debug_bounds(table_next).unwrap().bottom() < px(680.));
                click(visual, table_next);
            }
            wait(visual, |cx| {
                let tree = snapshot(&panel, cx);
                !tree.contains("poker-result-dialog")
                    && tree.contains(if match_over {
                        "poker-action-call"
                    } else {
                        "poker-action-check"
                    })
            });
            visual.update(|window, _| window.remove_window());
            drop(panel);
            fixture.close();
        }
    }
}

#[gpui::test]
fn constrains_raise_options_to_available_chips(cx: &mut TestAppContext) {
    init(cx);
    for (remote, stack) in [(false, 15), (true, 15), (false, 50), (true, 50)] {
        let fixture = Fixture::new(remote);
        let (_, server) = fixture.game_plugin_with("poker", &["player_1_model"], |root| {
            anchors(root);
            let path = root.join("dev.sailry.platform/desktop/main.js");
            let source = std::fs::read_to_string(&path).unwrap();
            std::fs::write(
                path,
                source.replace(
                    "this.game = deal();",
                    &format!(
                        "this.game = deal(Math.random, [{stack}, {}]);",
                        2000 - stack
                    ),
                ),
            )
            .unwrap();
        });
        let (panel, visual) = mount(&fixture, cx);
        open_package(visual, &panel, "poker");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("poker-status-ready")
        });
        click(visual, "poker-start");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("poker-action-call")
        });
        click(visual, "poker-open-raise");
        let tree = visual.update(|_, cx| snapshot(&panel, cx));
        assert!(tree.contains("poker-raise-dialog"));
        assert!(visual.debug_bounds("poker-all-in").is_some());
        if stack == 15 {
            for id in [
                "poker-min",
                "poker-raise-2x",
                "poker-raise-3x",
                "poker-raise-5x",
                "poker-raise-10x",
            ] {
                assert!(visual.debug_bounds(id).is_none());
            }
        } else {
            click(visual, "poker-raise-5x");
            click(visual, "poker-raise-10x");
        }
        assert!(visual.debug_bounds("poker-raise-input").is_none());
        assert!(server.requests.lock().unwrap().is_empty());
        click(visual, "poker-raise");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("poker-result-dialog")
        });
        let requests = server.requests.lock().unwrap();
        let prompt = requests[0]["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap();
        let turn: serde_json::Value = serde_json::from_str(prompt.lines().last().unwrap()).unwrap();
        assert_eq!(turn["state"]["history"][0]["to"], stack);
        drop(requests);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}
