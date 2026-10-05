use super::*;
use crate::{preview::Page, shell::Shell, tr};

mod board_games;
mod city_trader;
mod liars_dice;
mod poker;
mod reversi;
mod selection;
mod settings;
mod xiangqi;

#[gpui::test]
fn sidebar_opens_without_project(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (package, server) = fixture.game(true);
        cx.update(|cx| cx.set_global(fixture.services(false)));
        let selected = if remote {
            fixture.controller.id()
        } else {
            fixture.node.id()
        };
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| Shell::new(window, cx));
            owner = Some(shell.clone());
            Root::new(shell, window, cx)
        });
        let shell = owner.unwrap();
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .hosts
                .contains_key(&selected)
        });
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(selected, cx)
            })
        });
        wait(visual, |cx| {
            shell.read(cx).extension_entries(cx).iter().any(|entry| {
                entry.node == fixture.node.id() && entry.package == package.summary.reference()
            })
        });
        visual.update(|_, cx| {
            shell.update(cx, |shell, _| shell.live.as_mut().unwrap().project = None)
        });
        assert!(shell.read_with(visual, |shell, _| {
            shell.live.as_ref().unwrap().project.is_none()
        }));
        click(visual, "navigation-more");
        // Kit places the dropdown in a deferred frame before its rows can be clicked.
        for _ in 0..2 {
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
        }
        let selector = format!("more-navigation-plugin-{:?}-doudizhu", fixture.node.id());
        click(visual, Box::leak(selector.into_boxed_str()));
        let panel = shell.read_with(visual, |shell, _| {
            assert_eq!(shell.page, Page::Plugin);
            shell.extensions.as_ref().unwrap().panel.clone().unwrap()
        });
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("ddz-status-ready")
        });
        assert!(panel.read_with(visual, |panel, _| panel.binding.worktree.is_none()));
        assert_eq!(
            panel.read_with(visual, |panel, _| panel.binding.client.target()),
            fixture.node.id()
        );
        assert!(panel.read_with(visual, |panel, cx| panel.launcher_entries(cx).is_empty()));
        assert!(
            server.requests.lock().unwrap().is_empty(),
            "opening the plugin must not invoke a model"
        );
        fixture.execute(Command::SetPluginEnabled {
            name: "doudizhu".into(),
            expected_revision: package.summary.revision,
            enabled: false,
        });
        wait(visual, |cx| {
            panel.read(cx).mounted.is_none()
                && !shell
                    .read(cx)
                    .extension_entries(cx)
                    .iter()
                    .any(|entry| entry.package.name == "doudizhu")
        });
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.close();
    }
}

#[gpui::test]
fn recovers_model_receipts_and_retries_failed_turns(cx: &mut TestAppContext) {
    init(cx);
    rust_i18n::set_locale("zh-CN");
    for remote in [false, true] {
        for mode in [0, 5, 6] {
            let lost = mode != 0;
            let fixture = Fixture::new(remote);
            let (_, server) = fixture.game(true);
            let failed = fixture
                .runtime
                .block_on(crate::agent_fixture::Server::http(1, 503));
            if !lost {
                fixture.game_endpoint(&failed.endpoint);
            }
            let (panel, visual) = mount(&fixture, cx);
            let handle = visual.update(|window, _| window.window_handle());
            visual.simulate_window_resize(handle, size(px(1040.), px(820.)));
            open_package(visual, &panel, "doudizhu");
            wait(visual, |cx| {
                snapshot(&panel, cx).contains("ddz-status-ready")
            });
            click(visual, "ddz-deal");
            wait(visual, |cx| snapshot(&panel, cx).contains("ddz-status-bid"));
            click(visual, "ddz-bid-3");
            wait(visual, |cx| {
                snapshot(&panel, cx).contains("ddz-status-turn")
            });
            let tree = visual.update(|_, cx| snapshot(&panel, cx));
            let last = tree.rsplit("ddz-slot-").next().unwrap();
            let value: String = last.chars().take_while(char::is_ascii_digit).collect();
            click(visual, Box::leak(format!("card-{value}").into_boxed_str()));
            wait(visual, |cx| snapshot(&panel, cx).contains("ddz-selected-"));
            assert_eq!(
                visual
                    .update(|_, cx| snapshot(&panel, cx))
                    .matches("ddz-selected-")
                    .count(),
                1
            );
            // Keep the selected single card so both opponents have a choice.
            if lost {
                fixture.transport.mode.store(mode, Ordering::SeqCst);
            }
            click(visual, "ddz-play");
            if !lost {
                toast(visual, tr("chat_model_failed").as_ref());
                wait(visual, |cx| snapshot(&panel, cx).contains("ddz-retry"));
                let tree = visual.update(|_, cx| snapshot(&panel, cx));
                assert!(!tree.contains("ddz-error"));
                assert!(!tree.contains("ddz-controls"));
                assert_eq!(tree.matches("ddz-play-avatar-").count(), 1);
                assert!(tree.contains("ddz-play-avatar-0"));
                assert_eq!(failed.requests.lock().unwrap().len(), 1);
                assert!(server.requests.lock().unwrap().is_empty());
                fixture.game_endpoint(&server.endpoint);
                click(visual, "ddz-retry");
            }
            wait(visual, |cx| {
                snapshot(&panel, cx).contains("ddz-plays")
                    && snapshot(&panel, cx).contains("ddz-status-turn")
            });
            assert!(visual.update(|_, cx| !snapshot(&panel, cx).contains("ddz-error")));
            // Both seats act once; forced passes do not generate or replay a request.
            let requests = server.requests.lock().unwrap();
            assert!((1..=2).contains(&requests.len()));
            for (index, request) in requests.iter().enumerate() {
                let prompt = request["messages"].as_array().unwrap().last().unwrap()["content"]
                    .as_str()
                    .unwrap();
                let state: serde_json::Value =
                    serde_json::from_str(prompt.lines().last().unwrap()).unwrap();
                assert_eq!(state["state"]["player"], index + 1);
                assert!(state["state"]["hands"].is_null());
            }
            drop(requests);
            visual.update(|window, _| window.remove_window());
            drop(panel);
            fixture.close();
        }
    }
}

#[gpui::test]
fn requires_configuration_before_dealing(cx: &mut TestAppContext) {
    init(cx);
    rust_i18n::set_locale("en");
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (_, server) = fixture.game(false);
        let (panel, visual) = mount(&fixture, cx);
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1040.), px(820.)));
        open_package(visual, &panel, "doudizhu");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("ddz-status-configure")
        });
        let tree = visual.update(|_, cx| snapshot(&panel, cx));
        assert!(tree.contains("ddz-lobby"));
        assert!(
            !tree.contains("Button"),
            "unconfigured state must not offer game actions"
        );
        assert_eq!(
            tree.matches("Configure players in game settings").count(),
            1
        );
        for absent in ["ddz-table", "ddz-hand", "ddz-deal", "ddz-refresh", "tokens"] {
            assert!(!tree.contains(absent), "unexpected game content: {absent}");
        }
        assert!(server.requests.lock().unwrap().is_empty());
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn retries_a_failed_configuration_read(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (_, server) = fixture.game(true);
        let (panel, visual) = mount(&fixture, cx);
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1040.), px(820.)));
        fixture.transport.mode.store(4, Ordering::SeqCst);
        open_package(visual, &panel, "doudizhu");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("ddz-status-loadFailed")
        });
        toast(visual, "Could not load game settings");
        let tree = visual.update(|_, cx| snapshot(&panel, cx));
        assert!(!tree.contains("Could not load game settings"));
        assert_eq!(tree.matches("Button").count(), 1, "{tree}");
        assert!(!tree.contains("ddz-table"));
        assert!(!tree.contains("ddz-status-configure"));
        click(visual, "ddz-retry");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("ddz-status-ready")
        });
        assert!(server.requests.lock().unwrap().is_empty());
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn thinking_replaces_controls_below_the_last_play(cx: &mut TestAppContext) {
    init(cx);
    rust_i18n::set_locale("zh-CN");
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (_, _server) = fixture.game(true);
        let delayed = fixture
            .runtime
            .block_on(crate::agent_fixture::Server::markdown_after(
                "{\"move\":0}".into(),
                std::time::Duration::from_millis(1800),
            ));
        fixture.game_endpoint(&delayed.endpoint);
        let (panel, visual) = mount(&fixture, cx);
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1040.), px(820.)));
        open_package(visual, &panel, "doudizhu");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("ddz-status-ready")
        });
        click(visual, "ddz-deal");
        wait(visual, |cx| snapshot(&panel, cx).contains("ddz-status-bid"));
        click(visual, "ddz-bid-3");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("ddz-status-turn")
        });
        let tree = visual.update(|_, cx| snapshot(&panel, cx));
        let value: String = tree
            .rsplit("ddz-slot-")
            .next()
            .unwrap()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        click(visual, Box::leak(format!("card-{value}").into_boxed_str()));
        click(visual, "ddz-play");
        wait(visual, |cx| snapshot(&panel, cx).contains("text \"1 秒\""));
        let tree = visual.update(|_, cx| snapshot(&panel, cx));
        assert!(tree.contains("ddz-status-thinking"));
        assert!(tree.contains("ShimmerText"));
        assert!(tree.contains("ddz-elapsed"));
        assert!(tree.contains("ddz-play-avatar-0"));
        assert!(!tree.contains("ddz-controls"));
        let thinking = visual.debug_bounds("ddz-status-avatar").unwrap();
        assert!(thinking.top() > visual.debug_bounds("ddz-last-play-bottom").unwrap().top());
        assert!(thinking.bottom() < visual.debug_bounds("ddz-hand-top").unwrap().top());
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("ddz-status-turn")
        });
        let tree = visual.update(|_, cx| snapshot(&panel, cx));
        assert!(tree.contains("ddz-controls"));
        assert!(!tree.contains("ShimmerText"));
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn finishes_scores_and_redeals(cx: &mut TestAppContext) {
    init(cx);
    rust_i18n::set_locale("zh-CN");
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (_, server) = fixture.game(true);
        let (panel, visual) = mount(&fixture, cx);
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1040.), px(820.)));
        open_package(visual, &panel, "doudizhu");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("ddz-status-ready")
        });
        click(visual, "ddz-deal");
        wait(visual, |cx| snapshot(&panel, cx).contains("ddz-status-bid"));
        let portraits = ["ddz-avatar-1", "ddz-avatar-0", "ddz-avatar-2"];
        assert_player_strip(visual, &portraits);
        let before = visual.update(|_, cx| snapshot(&panel, cx));
        assert!(before.contains("ddz-active-0"));
        assert!(before.contains("ddz-bottom-hidden"));
        assert!(before.contains("ddz-history-list"));
        assert!(!before.contains("ddz-history-play-"));
        assert!(!before.contains("请选择叫分"));
        assert!(!before.contains("玩家 ·"));
        assert_eq!(before.matches("text \"你\"").count(), 1);
        click(visual, "ddz-bid-3");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("ddz-status-turn")
        });
        let mut previous_height = None;
        for (width, height) in [(960., 680.), (1040., 820.)] {
            visual.simulate_window_resize(handle, size(px(width), px(height)));
            assert_player_strip(visual, &portraits);
            let play = visual.debug_bounds("ddz-play").unwrap();
            assert!(play.bottom() < px(height) && play.right() < px(width));
            assert!(play.top() > visual.debug_bounds("ddz-last-play-bottom").unwrap().top());
            assert!(play.bottom() < visual.debug_bounds("ddz-hand-top").unwrap().top());
            let area_height = visual.debug_bounds("ddz-plays-bottom").unwrap().top()
                - visual.debug_bounds("ddz-plays-top").unwrap().top();
            assert!(area_height >= px(240.));
            if let Some(previous) = previous_height {
                assert!(area_height > previous + px(80.));
            }
            previous_height = Some(area_height);
        }
        let tree = visual.update(|_, cx| snapshot(&panel, cx));
        assert!(tree.contains("ddz-bottom-revealed"));
        assert!(tree.contains("ddz-active-0"));
        for index in 1..=3 {
            assert!(tree.contains(&format!("character-{index:02}.png")));
        }
        let finished =
            |tree: &str| tree.contains("ddz-result-win") || tree.contains("ddz-result-lose");
        let mut turns = 0;
        loop {
            let tree = visual.update(|_, cx| snapshot(&panel, cx));
            if finished(&tree) {
                break;
            }
            assert!(turns < 100, "game did not finish");
            assert!(!tree.contains("ddz-error"), "{tree}");
            assert!(!tree.contains("ddz-played-"));
            if turns > 0 {
                assert_eq!(tree.matches("ddz-play-avatar-").count(), 1);
                assert_eq!(tree.matches("ddz-cards-motion\"").count(), 1);
            }
            click(visual, "ddz-hint");
            let selected = visual.update(|_, cx| snapshot(&panel, cx).contains("ddz-selected-"));
            let before = visual.update(|_, cx| snapshot(&panel, cx));
            if !selected {
                let pass = before
                    .lines()
                    .skip_while(|line| !line.contains("ddz-controls"))
                    .filter(|line| line.contains("Button"))
                    .nth(1)
                    .unwrap();
                assert!(
                    pass.contains(":primary(registered)")
                        && pass.contains(":disabled[Bool(false)]")
                );
                assert!(!before.contains("ShimmerText"));
            }
            click(visual, if selected { "ddz-play" } else { "ddz-pass" });
            wait(visual, |cx| {
                let tree = snapshot(&panel, cx);
                finished(&tree) || (tree != before && tree.contains("ddz-status-turn"))
            });
            turns += 1;
        }
        let tree = visual.update(|_, cx| snapshot(&panel, cx));
        assert!(tree.contains("ddz-history-play-"));
        assert!(tree.contains("ddz-history-pattern-"));
        let number = |visual: &mut VisualTestContext, marker: &str| -> i64 {
            visual.update(|_, cx| {
                panel
                    .read(cx)
                    .mounted
                    .as_ref()
                    .unwrap()
                    .header
                    .read(cx)
                    .value(if marker.contains("score") { 0 } else { 1 })
                    .unwrap()
                    .trim_start_matches('×')
                    .parse()
                    .unwrap()
            })
        };
        let score = number(visual, "ddz-score\"");
        let multiplier = number(visual, "ddz-multiplier");
        assert_eq!(
            score,
            if tree.contains("ddz-result-win") {
                6
            } else {
                -6
            } * multiplier
        );
        assert!(!tree.contains("ddz-selected-"));
        assert!(tree.contains("ddz-result-overlay"));
        let before = server.requests.lock().unwrap().len();
        click(visual, "ddz-deal");
        wait(visual, |cx| snapshot(&panel, cx).contains("ddz-status-bid"));
        // The next round begins at the left seat; both AI players pass bidding.
        assert_eq!(server.requests.lock().unwrap().len(), before + 2);
        assert_eq!(number(visual, "ddz-score\""), score);
        click(visual, "ddz-bid-0");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("ddz-status-redeal")
        });
        assert_eq!(number(visual, "ddz-score\""), score);
        click(visual, "ddz-deal");
        wait(visual, |cx| snapshot(&panel, cx).contains("ddz-status-bid"));
        assert_eq!(server.requests.lock().unwrap().len(), before + 3);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

fn click_game(visual: &mut VisualTestContext, x: f32, y: f32) {
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let panel = visual.debug_bounds("plugin-panel").unwrap();
    visual.simulate_click(panel.origin + point(px(x), px(y)), Modifiers::default());
    visual.run_until_parked();
}

fn assert_player_strip(visual: &mut VisualTestContext, portraits: &[&'static str]) {
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let bounds: Vec<_> = portraits
        .iter()
        .map(|id| visual.debug_bounds(id).unwrap())
        .collect();
    for avatar in &bounds {
        assert_eq!(avatar.size, size(px(40.), px(40.)));
        assert_eq!(avatar.top(), bounds[0].top());
    }
    for pair in bounds.windows(2) {
        assert!(pair[0].right() < pair[1].left());
    }
}

#[gpui::test]
fn gomoku_plays_a_human_and_model_turn(cx: &mut TestAppContext) {
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
        fixture.game_plugin_with("gomoku", &["ai_model"], |root| {
            board_games::anchors(root, "gomoku")
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
        open_package(visual, &panel, "gomoku");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("gomoku-status-ready")
        });
        assert!(server.requests.lock().unwrap().is_empty());

        click(visual, "gomoku-start-black");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("gomoku-status-yourTurn")
        });
        assert!(visual.update(|_, cx| snapshot(&panel, cx).contains("gomoku-board")));
        for (width, height) in [(960., 680.), (1280., 820.)] {
            visual.simulate_window_resize(handle, size(px(width), px(height)));
            board_games::layout(visual, "gomoku");
        }
        click(visual, "gomoku-cell-7-7");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("gomoku-status-thinking")
        });
        assert!(visual.debug_bounds("gomoku-status-avatar").is_some());
        wait(visual, |cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("gomoku-stone-7-7")
                && tree.matches("gomoku-stone-").count() == 2
                && tree.contains("gomoku-status-yourTurn")
        });
        let tree = visual.update(|_, cx| snapshot(&panel, cx));
        assert!(tree.contains("gomoku-history-avatar-2"));
        assert!(tree.find("gomoku-move-2").unwrap() < tree.find("gomoku-move-1").unwrap());
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let prompt = requests[0]["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap();
        let choice: serde_json::Value =
            serde_json::from_str(prompt.lines().last().unwrap()).unwrap();
        assert_eq!(choice["state"]["turn"], "O");
        assert_eq!(
            choice["state"]["board"][7].as_str().unwrap().chars().nth(7),
            Some('X')
        );
        drop(requests);

        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}
