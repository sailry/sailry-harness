use super::*;

#[gpui::test]
fn constrains_shell_and_presents_events(cx: &mut TestAppContext) {
    init(cx);
    let surfaces = std::rc::Rc::new(std::cell::Cell::new(0));
    cx.update({
        let surfaces = surfaces.clone();
        move |cx| {
            gpui_kit::component::surface::set_renderer(
                move |child, _, _| {
                    surfaces.set(surfaces.get() + 1);
                    div()
                        .debug_selector(|| "plugin-modal-surface".into())
                        .child(child)
                        .into_any_element()
                },
                cx,
            )
        }
    });
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (package, _server) = fixture.game_plugin_with("city-trader", &["player_1_model", "player_2_model"], |root| {
            for (file, before, after) in [
                ("game.js", "random = Math.random", "random = () => 0"),
                ("main.js", "this.game = newGame();", "this.game = newGame(); this.game.players[0].position = 2; this.game.players[1].position = 1; this.game.players[2].position = 3; this.game.history.push({key: 'roll', seat: 0, dice: [4, 4]}, {key: 'bonus', seat: 1, tile: 12, amount: 150});"),
            ] {
                let path = root.join("dev.sailry.platform/desktop").join(file);
                let source = std::fs::read_to_string(&path).unwrap();
                assert!(source.contains(before));
                std::fs::write(path, source.replace(before, after)).unwrap();
            }
            let path = root.join("dev.sailry.platform/desktop/controls.js");
            let source = std::fs::read_to_string(&path).unwrap();
            std::fs::write(path, format!("import {{ Anchor as TestAnchor }} from \"sailry/test\";\n{}", source.replace("return primary ? control.primary() : control.outline();", "return TestAnchor.new(id).child(primary ? control.primary() : control.outline());"))).unwrap();
            let path = root.join("dev.sailry.platform/desktop/portfolio.js");
            let source = std::fs::read_to_string(&path).unwrap()
                .replace(".id(`city-event-${index}`)", ".id(`city-event-${index}`).relative().child(div().absolute().right(0).top(0).child(TestAnchor.new(`history-end-${index}`)))")
                .replace(".accessibility_label(item.label)", ".child(div().absolute().left(16).top(16).child(TestAnchor.new(`history-icon-center-${index}`))).accessibility_label(item.label)");
            std::fs::write(path, format!("import {{ Anchor as TestAnchor }} from \"sailry/test\";\n{source}")).unwrap();
            let path = root.join("dev.sailry.platform/desktop/details.js");
            let source = std::fs::read_to_string(&path).unwrap();
            std::fs::write(path, format!("import {{ Anchor as TestAnchor }} from \"sailry/test\";\n{}", source.replace(".child(tileName(text, tile))", ".child(TestAnchor.new(\"city-detail-title\").child(tileName(text, tile)))").replace(".child(price)", ".child(TestAnchor.new(\"detail-price-text\").child(price))").replace(".child(label)", ".child(TestAnchor.new(`detail-label-${label}`).child(label))").replace(".child(value)", ".child(TestAnchor.new(`detail-value-${label}`).child(value))"))).unwrap();
        });
        cx.update(|cx| cx.set_global(fixture.services(false)));
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| Shell::new(window, cx));
            shell.update(cx, |shell, _| shell.layout.sidebar_width = 200.);
            owner = Some(shell.clone());
            Root::new(shell, window, cx)
        });
        let shell = owner.unwrap();
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(
            handle,
            size(px(1500. + crate::preview::RAIL_WIDTH), px(1000.)),
        );
        wait(visual, |cx| {
            shell
                .read(cx)
                .extension_entries(cx)
                .iter()
                .any(|entry| entry.package == package.summary.reference())
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let entry = shell
                    .extension_entries(cx)
                    .into_iter()
                    .find(|entry| entry.package == package.summary.reference())
                    .unwrap();
                shell.open_extension(entry, window, cx);
            })
        });
        let panel = shell.read_with(visual, |shell, _| {
            shell.extensions.as_ref().unwrap().panel.clone().unwrap()
        });
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("city-status-ready")
        });
        assert_eq!(
            panel.read_with(visual, |panel, cx| panel.min_content_width(cx)),
            px(0.)
        );
        click(visual, "city-start");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("city-state-roll-0-1")
        });
        assert!(!visual.update(|_, cx| snapshot(&panel, cx).contains("city-history-value-1")));
        assert!(visual.update(|_, cx| snapshot(&panel, cx).contains("8 steps")));
        let required = panel.read_with(visual, |panel, cx| panel.min_content_width(cx));
        assert_eq!(required, px(1270.));
        let required_height = panel.read_with(visual, |panel, cx| panel.min_content_height(cx));
        assert_eq!(required_height, px(769.));
        visual.update(|window, cx| window.draw(cx).clear(cx));
        for (avatar, icon, end) in [
            (
                "city-history-avatar-0",
                "history-icon-center-0",
                "history-end-0",
            ),
            (
                "city-history-avatar-1",
                "history-icon-center-1",
                "history-end-1",
            ),
        ] {
            let avatar = visual.debug_bounds(avatar).unwrap();
            let icon = visual.debug_bounds(icon).unwrap().origin;
            let end = visual.debug_bounds(end).unwrap().origin;
            assert!(
                (avatar.center().y - icon.y).abs() <= px(1.),
                "activity avatar and icon must align"
            );
            assert!(
                icon.x + px(16.) <= avatar.left(),
                "activity icon must precede the avatar"
            );
            assert!(
                (end.x - avatar.right()).abs() <= px(1.),
                "activity avatars must sit at the right edge"
            );
        }
        let trigger = visual.debug_bounds("city-history-tooltip-0").unwrap();
        visual.simulate_mouse_move(trigger.center(), None, Modifiers::default());
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            visual.executor().advance_clock(Duration::from_millis(100));
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            if visual
                .debug_bounds("plugin-tooltip-collected a bonus")
                .is_some()
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "activity tooltip must open on hover"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let bounds = visual.debug_bounds("main-region").unwrap();
        assert!(visual.debug_bounds("shell-navigation").is_none());
        assert_eq!(bounds.left(), px(crate::preview::RAIL_WIDTH));
        let start = point(bounds.left(), bounds.center().y);
        visual.simulate_mouse_move(start, None, Modifiers::default());
        visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        for step in 1..=4 {
            visual.simulate_mouse_move(
                point(start.x + px(step as f32 * 30.), start.y),
                MouseButton::Left,
                Modifiers::default(),
            );
            visual.update(|window, cx| window.draw(cx).clear(cx));
        }
        visual.simulate_mouse_up(
            point(start.x + px(120.), start.y),
            MouseButton::Left,
            Modifiers::default(),
        );
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let resized = visual.debug_bounds("main-region").unwrap();
        assert_eq!(
            resized.left(),
            bounds.left(),
            "a page without navigation must preserve the fixed rail width"
        );
        assert!(
            resized.size.width >= required - px(1.),
            "the game width must constrain pane dragging: {resized:?}"
        );
        visual.simulate_window_resize(handle, size(px(1000.), px(1000.)));
        visual.update(|window, cx| window.draw(cx).clear(cx));
        visual.run_until_parked();
        let restored = visual.update(|window, _| window.bounds().size);
        assert!(
            restored.width >= required + px(crate::preview::RAIL_WIDTH) - px(1.),
            "the declared plugin minimum must resize the Shell window: {restored:?}"
        );
        visual.simulate_window_resize(handle, restored);
        visual.update(|window, cx| window.draw(cx).clear(cx));
        visual.simulate_window_resize(handle, size(restored.width, px(600.)));
        visual.update(|window, cx| window.draw(cx).clear(cx));
        visual.run_until_parked();
        let restored = visual.update(|window, _| window.bounds().size);
        assert!(
            restored.height >= required_height + px(crate::preview::HEADER_HEIGHT),
            "the declared plugin height must constrain the Shell window: {restored:?}"
        );
        visual.simulate_window_resize(handle, restored);
        visual.update(|window, cx| window.draw(cx).clear(cx));
        click(visual, "city-action-roll-button");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("city-event-dialog")
        });
        let popup = visual.debug_bounds("plugin-modal-surface").unwrap();
        let coin = visual.debug_bounds("city-event-coin").unwrap();
        assert!((coin.center().x - popup.center().x).abs() <= px(1.));
        assert!(
            surfaces.get() > 0,
            "plugin dialogs must use the shared material renderer"
        );
        wait(visual, |cx| {
            !snapshot(&panel, cx).contains("city-event-dialog")
        });
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("city-state-roll-0-2")
        });
        assert!(!visual.update(|_, cx| snapshot(&panel, cx).contains("city-action-end-button")));
        click(visual, "city-building-art-1");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("city-property-dialog")
        });
        let popup = visual.debug_bounds("plugin-modal-surface").unwrap();
        let art = visual.debug_bounds("city-detail-art").unwrap();
        assert!((art.center().x - popup.center().x).abs() <= px(1.));
        assert!(visual.debug_bounds("city-detail-title").unwrap().top() >= art.bottom());
        let price = visual.debug_bounds("detail-price-text").unwrap();
        let coin = visual.debug_bounds("city-detail-coin").unwrap();
        assert!(
            ((coin.left() + price.right()) / 2. - art.center().x).abs() <= px(1.),
            "the coin and price must align with the centered property artwork"
        );
        assert!((coin.center().y - price.center().y).abs() <= px(1.));
        let mut centers = Vec::new();
        for (label, value, coin) in [
            (
                "detail-label-Rent",
                "detail-value-Rent",
                Some("city-detail-stat-coin-0"),
            ),
            ("detail-label-Buildings", "detail-value-Buildings", None),
            (
                "detail-label-Build cost",
                "detail-value-Build cost",
                Some("city-detail-stat-coin-2"),
            ),
        ] {
            let label = visual.debug_bounds(label).unwrap();
            let value = visual.debug_bounds(value).unwrap();
            let left = coin
                .map(|id| visual.debug_bounds(id).unwrap().left())
                .unwrap_or(value.left());
            assert!(
                (label.center().x - (left + value.right()) / 2.).abs() <= px(1.),
                "each statistic must center its label and value"
            );
            centers.push(label.center().x);
        }
        assert!((centers[1] - art.center().x).abs() <= px(1.));
        assert!(
            ((centers[1] - centers[0]) - (centers[2] - centers[1])).abs() <= px(1.),
            "statistics must occupy three equal columns"
        );
        visual.simulate_keystrokes("escape");
        wait(visual, |cx| {
            !snapshot(&panel, cx).contains("city-property-dialog")
        });
        click(visual, "city-header-new-game");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("city-status-ready")
        });
        assert_eq!(
            panel.read_with(visual, |panel, cx| panel.min_content_width(cx)),
            px(0.)
        );
        assert_eq!(
            panel.read_with(visual, |panel, cx| panel.min_content_height(cx)),
            px(0.)
        );
        visual.simulate_window_resize(handle, size(px(900.), px(600.)));
        visual.update(|window, cx| window.draw(cx).clear(cx));
        visual.run_until_parked();
        assert_eq!(
            visual.update(|window, _| window.bounds().size.width),
            px(900.),
            "leaving the board must release its window minimum"
        );
        assert_eq!(
            visual.update(|window, _| window.bounds().size.height),
            px(600.),
            "leaving the board must release its minimum height"
        );
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.close();
    }
}

#[gpui::test]
fn centers_content_and_scrolls_when_needed(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (_, _server) = fixture.game_plugin_with("city-trader", &["player_1_model", "player_2_model"], |root| {
            for (file, target, name) in [
                ("layout.js", ".id(\"city-stage\")", "stage"),
                ("lobby.js", ".id(\"city-lobby\")", "content"),
                ("view.js", ".id(`city-state-${game.phase}-${game.turn}-${game.round}`)", "content"),
            ] {
                let path = root.join("dev.sailry.platform/desktop").join(file);
                let source = std::fs::read_to_string(&path).unwrap();
                assert!(source.contains(target));
                let measured = format!("{target}.relative().child(div().absolute().top(0).left(0).child(TestAnchor.new(\"{name}-start\"))).child(div().absolute().bottom(0).right(0).child(TestAnchor.new(\"{name}-end\")))");
                std::fs::write(path, format!("import {{ Anchor as TestAnchor }} from \"sailry/test\";\n{}", source.replace(target, &measured))).unwrap();
            }
            let path = root.join("dev.sailry.platform/desktop/controls.js");
            let source = std::fs::read_to_string(&path).unwrap();
            std::fs::write(path, format!("import {{ Anchor as TestAnchor }} from \"sailry/test\";\n{}", source.replace("return primary ? control.primary() : control.outline();", "return TestAnchor.new(id).child(primary ? control.primary() : control.outline());"))).unwrap();
        });
        let (panel, visual) = mount(&fixture, cx);
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1800.), px(1200.)));
        open_package(visual, &panel, "city-trader");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("city-status-ready")
        });
        for playing in [false, true] {
            if playing {
                click(visual, "city-start");
                wait(visual, |cx| {
                    snapshot(&panel, cx).contains("city-state-roll-0-1")
                });
            }
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let stage_start = visual.debug_bounds("stage-start").unwrap().origin;
            let stage_end = visual.debug_bounds("stage-end").unwrap().origin;
            let start = visual.debug_bounds("content-start").unwrap().origin;
            let end = visual.debug_bounds("content-end").unwrap().origin;
            assert!(
                ((start.y + end.y) - (stage_start.y + stage_end.y)).abs() <= px(2.),
                "vertical centering: {start:?}, {end:?}, {stage_start:?}, {stage_end:?}"
            );
            assert!(((start.x + end.x) - (stage_start.x + stage_end.x)).abs() <= px(2.));
            assert!(end.x - start.x <= px(1240.));
            let position = point(stage_start.x + px(8.), stage_start.y + px(8.));
            visual.simulate_event(ScrollWheelEvent {
                position,
                delta: ScrollDelta::Pixels(point(px(0.), px(-1000.))),
                ..Default::default()
            });
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert_eq!(
                visual.debug_bounds("content-start").unwrap().origin,
                start,
                "fitting content must not scroll"
            );
            let pattern = visual.debug_bounds("city-pattern-10").unwrap();
            assert!(
                pattern.top() < stage_end.y && pattern.bottom() >= stage_end.y,
                "background must tile to the bottom of the stage"
            );
            visual.simulate_window_resize(handle, size(px(1280.), px(420.)));
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let before = visual.debug_bounds("content-start").unwrap().origin;
            let top = visual.debug_bounds("stage-start").unwrap().origin;
            assert!(
                before.y >= top.y,
                "overflowing content must start within reach"
            );
            visual.simulate_event(ScrollWheelEvent {
                position: point(top.x + px(8.), top.y + px(8.)),
                delta: ScrollDelta::Pixels(point(px(0.), px(-2000.))),
                ..Default::default()
            });
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert!(visual.debug_bounds("content-start").unwrap().top() < before.y);
            assert!(
                visual.debug_bounds("content-end").unwrap().top()
                    <= visual.debug_bounds("stage-end").unwrap().top()
            );
            if playing {
                visual.simulate_window_resize(handle, size(px(760.), px(700.)));
                visual.update(|window, cx| window.draw(cx).clear(cx));
                let stage = visual.debug_bounds("stage-start").unwrap().origin;
                let before = visual.debug_bounds("content-start").unwrap().origin;
                assert!(
                    before.x >= stage.x,
                    "leftmost content must remain reachable"
                );
                visual.simulate_event(ScrollWheelEvent {
                    position: point(stage.x + px(8.), stage.y + px(8.)),
                    delta: ScrollDelta::Pixels(point(px(-2000.), px(0.))),
                    ..Default::default()
                });
                visual.update(|window, cx| window.draw(cx).clear(cx));
                assert!(visual.debug_bounds("content-start").unwrap().left() < before.x);
                assert!(
                    visual.debug_bounds("content-end").unwrap().left()
                        <= visual.debug_bounds("stage-end").unwrap().left()
                );
            }
            visual.simulate_window_resize(handle, size(px(1800.), px(1200.)));
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert_eq!(
                visual.debug_bounds("content-start").unwrap().origin,
                start,
                "growing the window must restore centering"
            );
        }
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn auctions_and_collects_rent(cx: &mut TestAppContext) {
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
        let (_, server) = fixture.game_plugin_with(
            "city-trader",
            &["player_1_model", "player_2_model"],
            |root| {
                // Fix the die source in the isolated package fixture.
                let path = root.join("dev.sailry.platform/desktop/game.js");
                let source = std::fs::read_to_string(&path).unwrap();
                assert!(source.contains("random = Math.random"));
                std::fs::write(
                    path,
                    source.replace("random = Math.random", "random = () => 0"),
                )
                .unwrap();
                let path = root.join("dev.sailry.platform/desktop/data.js");
                let source = std::fs::read_to_string(&path).unwrap();
                assert!(source.contains("ROUND_LIMIT = 20"));
                std::fs::write(path, source.replace("ROUND_LIMIT = 20", "ROUND_LIMIT = 2"))
                    .unwrap();
                // The dedicated event test covers the real presentation delay.
                let path = root.join("dev.sailry.platform/desktop/motion.js");
                let source = std::fs::read_to_string(&path).unwrap();
                assert!(source.contains("cx.sleep(2000)"));
                std::fs::write(path, source.replace("cx.sleep(2000)", "cx.sleep(30)")).unwrap();
                let path = root.join("dev.sailry.platform/desktop/view.js");
                let source = std::fs::read_to_string(&path).unwrap();
                let source = source
                    .replace(".id(\"city-player-strip\")", ".id(\"city-player-strip\").child(div().absolute().bottom(0).left(0).child(TestAnchor.new(\"city-strip-bottom\")))")
                    .replace(".id(\"city-left-region\")", ".id(\"city-left-region\").relative().child(div().absolute().top(0).right(0).child(TestAnchor.new(\"city-left-top\"))).child(div().absolute().bottom(0).right(0).child(TestAnchor.new(\"city-left-bottom\")))")
                    .replace(".id(\"city-right-region\")", ".id(\"city-right-region\").relative().child(div().absolute().top(0).left(0).child(TestAnchor.new(\"city-right-top\"))).child(div().absolute().bottom(0).left(0).child(TestAnchor.new(\"city-right-bottom\")))");
                std::fs::write(path, format!("import {{ Anchor as TestAnchor }} from \"sailry/test\";\n{source}")).unwrap();
                let path = root.join("dev.sailry.platform/desktop/board.js");
                let source = std::fs::read_to_string(&path).unwrap();
                std::fs::write(path, format!("import {{ Anchor as TestAnchor }} from \"sailry/test\";\n{}",
                    source.replace(".child(tileName(text, item))",
                        ".child(TestAnchor.new(`city-label-${item.index}`).child(tileName(text, item)))")))
                    .unwrap();
                // The pinned script bridge does not expose control bounds. A test-only
                // host anchor measures the actual Kit button without intercepting input.
                let path = root.join("dev.sailry.platform/desktop/controls.js");
                let source = std::fs::read_to_string(&path).unwrap();
                std::fs::write(path, format!("import {{ Anchor as TestAnchor }} from \"sailry/test\";\n{}",
                    source.replace("return primary ? control.primary() : control.outline();",
                        "return TestAnchor.new(id).child(primary ? control.primary() : control.outline());")))
                    .unwrap();
            },
        );
        let (panel, visual) = mount(&fixture, cx);
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
        open_package(visual, &panel, "city-trader");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("city-status-ready")
        });
        assert!(server.requests.lock().unwrap().is_empty());
        let image =
            panel.read_with(visual, |panel, _| {
                panel.mounted.as_ref().unwrap().cache_root().join(
                    "resources/dev.sailry.platform/desktop/assets/buildings/01-canal-walk.png",
                )
            });
        let decoded = image::open(image).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (220, 220));
        let pattern = panel.read_with(visual, |panel, _| {
            panel
                .mounted
                .as_ref()
                .unwrap()
                .cache_root()
                .join("resources/dev.sailry.platform/desktop/assets/interface/town-pattern.png")
        });
        let pattern = image::open(pattern).unwrap().into_rgba8();
        assert_eq!(pattern.width(), pattern.height());
        let last = pattern.width() - 1;
        for index in 0..=last {
            // At most one alpha level of antialiasing residue on tile edges.
            for (x, y) in [(index, 0), (index, last), (0, index), (last, index)] {
                assert!(pattern.get_pixel(x, y)[3] <= 1);
            }
        }

        click(visual, "city-character-art-4");
        click(visual, "city-start");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("city-state-roll-0-1")
        });
        assert!(visual.update(|_, cx| {
            let tree = snapshot(&panel, cx);
            assert!((0..24).all(|index| tree.contains(&format!("city-tile-{index}"))));
            tree.lines()
                .any(|line| line.contains("city-avatar-0") && line.contains("character-05.png"))
        }));
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let strip = visual.debug_bounds("city-strip-bottom").unwrap();
        let left = visual.debug_bounds("city-left-top").unwrap();
        let right = visual.debug_bounds("city-right-top").unwrap();
        assert_eq!(left.top(), right.top());
        assert!(
            (left.top() - strip.top()).abs() <= px(1.),
            "section rules must join the strip"
        );
        assert_eq!(
            visual.debug_bounds("city-left-bottom").unwrap().top(),
            visual.debug_bounds("city-right-bottom").unwrap().top()
        );
        let avatars = ["city-avatar-0", "city-avatar-1", "city-avatar-2"]
            .map(|id| visual.debug_bounds(id).unwrap());
        assert_eq!(avatars[0].top(), avatars[1].top());
        assert_eq!(avatars[1].top(), avatars[2].top());
        assert!(
            ((avatars[1].left() - avatars[0].left()) - (avatars[2].left() - avatars[1].left()))
                .abs()
                <= px(1.)
        );
        let coin = visual.debug_bounds("city-balance-0-1200-icon").unwrap();
        let gem = visual.debug_bounds("city-worth-0-icon").unwrap();
        assert_eq!(coin.top(), gem.top());
        assert_eq!(coin.size, gem.size);
        let pattern_size = visual.debug_bounds("city-pattern-0").unwrap().size;
        let top_left = visual.debug_bounds("city-building-art-0").unwrap();
        let top_right = visual.debug_bounds("city-building-art-6").unwrap();
        let bottom_right = visual.debug_bounds("city-building-art-12").unwrap();
        let bottom_left = visual.debug_bounds("city-building-art-18").unwrap();
        assert_eq!(top_left.origin.y, top_right.origin.y);
        assert_eq!(bottom_left.origin.y, bottom_right.origin.y);
        assert_eq!(top_left.origin.x, bottom_left.origin.x);
        assert_eq!(top_right.origin.x, bottom_right.origin.x);
        let board_center = (top_left.center().x + top_right.center().x) / 2.;
        assert!((board_center - (left.left() + right.left()) / 2.).abs() <= px(1.));
        let die = visual.debug_bounds("city-die-art-0").unwrap();
        assert!(die.left() > top_left.right() && die.right() < top_right.left());
        assert!(die.top() > top_left.bottom() && die.bottom() < bottom_left.top());
        for index in 0..24 {
            let art = visual
                .debug_bounds(Box::leak(
                    format!("city-building-art-{index}").into_boxed_str(),
                ))
                .unwrap();
            let label = visual
                .debug_bounds(Box::leak(format!("city-label-{index}").into_boxed_str()))
                .unwrap();
            assert!(
                art.bottom() <= label.top(),
                "tile {index}: {art:?} overlaps {label:?}"
            );
        }
        let first = visual.debug_bounds("city-portfolio-avatar-0").unwrap();
        let last = visual.debug_bounds("city-portfolio-avatar-2").unwrap();
        assert!(
            last.top() - first.top() < px(180.),
            "empty portfolios must stay compact"
        );
        visual.simulate_window_resize(handle, size(px(1420.), px(900.)));
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(
            visual.debug_bounds("city-pattern-0").unwrap().size,
            pattern_size
        );
        let pattern_next = visual.debug_bounds("city-pattern-1").unwrap();
        assert_eq!(
            pattern_next.left(),
            visual.debug_bounds("city-pattern-0").unwrap().right()
        );
        visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
        click(visual, "city-avatar-2");
        assert!(visual.update(|_, cx| snapshot(&panel, cx).contains("city-portfolio-2-closed")));
        click(visual, "city-portfolio-avatar-2");
        assert!(visual.update(|_, cx| snapshot(&panel, cx).contains("city-portfolio-2-open")));
        click(visual, "city-action-roll-button");
        wait(visual, |cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("city-state-buy-0-1") && tree.contains("city-status-yourTurn")
        });
        assert!(visual.update(|_, cx| snapshot(&panel, cx).contains("city-property-dialog")));
        click(visual, "city-close-details");
        assert!(visual.update(|_, cx| !snapshot(&panel, cx).contains("city-property-dialog")));
        assert!(visual.update(|_, cx| snapshot(&panel, cx).contains("city-state-buy-0-1")));
        click(visual, "city-open-decision");
        visual.simulate_keystrokes("escape");
        wait(visual, |cx| {
            !snapshot(&panel, cx).contains("city-property-dialog")
        });
        click(visual, "city-open-decision");
        fixture.transport.mode.store(8, Ordering::SeqCst);
        click(visual, "city-action-decline-button");
        wait(visual, |_| fixture.transport.entered.is_cancelled());
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("city-status-thinking")
        });
        assert!(visual.update(|_, cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("city-status-avatar")
                && !tree.contains("city-status-player")
                && tree.contains("city-auction-leader-null")
        }));
        fixture.transport.release.cancel();
        wait(visual, |cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("city-state-auction-0-1") && tree.contains("city-status-yourTurn")
        });
        assert!(visual.update(|_, cx| snapshot(&panel, cx).contains("city-auction-leader-null")));
        click(visual, "city-close-details");
        click(visual, "city-building-art-1");
        assert!(visual.update(|_, cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("city-selected-1")
                && !tree.contains("city-action-list")
                && !tree.contains("city-detail-participant")
        }));
        click(visual, "city-close-details");
        click(visual, "city-auction-art");
        assert!(visual.update(|_, cx| snapshot(&panel, cx).contains("city-selected-2")));
        click(visual, "city-action-bid-button");
        wait(visual, |cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("city-state-roll-0-2")
                && tree.contains("city-list-art-2")
                && tree.contains("city-status-yourTurn")
        });
        assert!(visual.update(|_, cx| !snapshot(&panel, cx).contains("city-auction-leader-")));
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 4);
        for (index, request) in requests.iter().enumerate() {
            let prompt = request["messages"].as_array().unwrap().last().unwrap()["content"]
                .as_str()
                .unwrap();
            let turn: serde_json::Value =
                serde_json::from_str(prompt.lines().last().unwrap()).unwrap();
            assert_eq!(
                turn["state"]["phase"],
                if index < 2 { "auction" } else { "manage" }
            );
            assert_eq!(turn["state"]["turn"], index % 2 + 1);
            if index < 2 {
                assert_eq!(turn["state"]["auction"]["tile"], 2);
            }
            assert_eq!(turn["state"]["board"].as_array().unwrap().len(), 24);
            assert_eq!(
                turn["choices"][0]["kind"],
                if index < 2 { "pass" } else { "end" }
            );
        }
        drop(requests);
        assert!(panel.read_with(visual, |_, cx| {
            snapshot(&panel, cx).contains("city-balance-0-1064")
        }));
        click(visual, "city-header-save");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("city-table-save-1")
        });
        toast(visual, "Saved");
        visual.update(|window, cx| window.clear_notifications(cx));
        crate::feedback::tests::settle(visual);
        visual.update(|_, cx| panel.update(cx, |panel, cx| panel.back(cx)));
        wait(visual, |cx| panel.read(cx).mounted.is_none());
        open_package(visual, &panel, "city-trader");
        assert_eq!(
            panel.read_with(visual, |panel, _| panel
                .selected
                .as_ref()
                .map(|package| package.name.as_str())
                .map(str::to_owned)),
            Some("city-trader".into())
        );
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("city-status-ready")
        });
        click(visual, "city-resume");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("city-state-roll-0-2")
        });
        assert!(panel.read_with(visual, |_, cx| {
            snapshot(&panel, cx).contains("city-list-art-2")
        }));
        assert!(panel.read_with(visual, |_, cx| {
            snapshot(&panel, cx).contains("city-balance-0-1064")
        }));
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        assert!(visual.update(|_, cx| {
            snapshot(&panel, cx)
                .lines()
                .any(|line| line.contains("city-avatar-0") && line.contains("character-05.png"))
        }));
        click(visual, "city-action-roll-button");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("city-result-dialog")
        });
        assert!(visual.update(|_, cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("city-outcome-victory")
                && (0..3).all(|seat| tree.contains(&format!("city-result-seat-{seat}")))
        }));
        click(visual, "city-close-result");
        click(visual, "city-building-art-2");
        assert!(visual.update(|_, cx| snapshot(&panel, cx).contains("city-property-dialog")));
        click(visual, "city-close-details");
        click(visual, "city-open-result");
        assert!(visual.update(|_, cx| snapshot(&panel, cx).contains("city-result-dialog")));
        click(visual, "city-again");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("city-status-ready")
        });
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn reviews_property_offers(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (_, server) = fixture.game_plugin_with(
            "city-trader",
            &["player_1_model", "player_2_model"],
            |root| {
                let path = root.join("dev.sailry.platform/desktop/main.js");
                let source = std::fs::read_to_string(&path).unwrap();
                std::fs::write(path, source.replace("this.game = newGame();", r#"
                    this.game = newGame(() => 0);
                    this.game.properties[1].owner = 0;
                    this.game.active = 1;
                    this.game.phase = "trade";
                    this.game.trade = { tile: 1, to: 0, price: 260 };
                    this.game.history.push({ key: "trade_proposed", seat: 1, tile: 1, price: 260 });
                "#)).unwrap();
                let path = root.join("dev.sailry.platform/desktop/controls.js");
                let source = std::fs::read_to_string(&path).unwrap();
                std::fs::write(path, format!("import {{ Anchor as TestAnchor }} from \"sailry/test\";\n{}",
                    source.replace("return primary ? control.primary() : control.outline();",
                        "return TestAnchor.new(id).child(primary ? control.primary() : control.outline());")))
                    .unwrap();
            },
        );
        let (panel, visual) = mount(&fixture, cx);
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
        open_package(visual, &panel, "city-trader");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("city-status-ready")
        });
        click(visual, "city-start");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("city-property-dialog")
        });
        assert!(visual.update(|_, cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("city-selected-1")
                && tree.contains("city-detail-participant")
                && tree.contains("260")
                && !tree.contains("$")
                && tree.contains("city-action-accept-button")
                && tree.contains("city-action-reject-button")
        }));
        let accept = visual.debug_bounds("city-action-accept-button").unwrap();
        let reject = visual.debug_bounds("city-action-reject-button").unwrap();
        let art = visual.debug_bounds("city-detail-art").unwrap();
        let participant = visual.debug_bounds("city-detail-avatar").unwrap();
        let close = visual.debug_bounds("city-close-details").unwrap();
        assert!((participant.center().y - close.center().y).abs() <= px(1.));
        assert!(participant.bottom() < art.top());
        assert!(visual.update(|_, cx| snapshot(&panel, cx).contains("city-owner-avatar-1")));
        assert!((accept.size.width - px(196.)).abs() <= px(1.));
        assert!((accept.size.width - reject.size.width).abs() <= px(1.));
        assert!((reject.left() - accept.right() - px(8.)).abs() <= px(1.));
        assert!(((accept.left() + reject.right()) / 2. - art.center().x).abs() <= px(1.));
        assert!(
            !visual.update(|_, cx| snapshot(&panel, cx).contains("Buyer")),
            "offers must not repeat the buyer role beside the avatar"
        );
        click(visual, "city-close-details");
        click(visual, "city-building-art-2");
        assert!(visual.update(|_, cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("city-selected-2")
                && !tree.contains("city-detail-participant")
                && !tree.contains("city-action-list")
        }));
        click(visual, "city-close-details");
        click(visual, "city-open-decision");
        assert!(visual.update(|_, cx| snapshot(&panel, cx).contains("city-selected-1")));
        assert!(server.requests.lock().unwrap().is_empty());
        click(visual, "city-action-accept-button");
        assert!(visual.update(|_, cx| { snapshot(&panel, cx).contains("city-balance-0-1460") }));
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("city-state-roll-0-2")
        });
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}
