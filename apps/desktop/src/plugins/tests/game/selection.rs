use super::*;

fn position(visual: &mut VisualTestContext, card: u8) -> gpui::Point<gpui::Pixels> {
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = visual
        .debug_bounds(Box::leak(format!("card-{card}").into_boxed_str()))
        .unwrap();
    point(bounds.left() + px(2.), bounds.bottom() - px(20.))
}

fn tap(visual: &mut VisualTestContext, card: u8) {
    let point = position(visual, card);
    visual.simulate_click(point, Modifiers::default());
    visual.run_until_parked();
}

fn drag(visual: &mut VisualTestContext, from: u8, to: u8, outside: bool) {
    let start = position(visual, from);
    let end = position(visual, to);
    visual.simulate_mouse_move(start, None, Modifiers::default());
    visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_move(end, MouseButton::Left, Modifiers::default());
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let release = if outside {
        point(px(20.), px(20.))
    } else {
        end
    };
    visual.simulate_mouse_move(release, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_up(release, MouseButton::Left, Modifiers::default());
    visual.run_until_parked();
}

#[gpui::test]
fn preserves_selection_during_turns(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (_, _server) = fixture.game(true);
        let delayed = fixture
            .runtime
            .block_on(crate::agent_fixture::Server::markdown_after(
                "{\"move\":0}".into(),
                Duration::from_millis(3200),
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
            let tree = snapshot(&panel, cx);
            tree.contains("ddz-status-turn") && !tree.contains("ddz-dealing")
        });
        let values = |tree: &str| -> Vec<u8> {
            tree.split("ddz-slot-")
                .skip(1)
                .map(|tail| {
                    tail.chars()
                        .take_while(char::is_ascii_digit)
                        .collect::<String>()
                        .parse()
                        .unwrap()
                })
                .collect()
        };
        let cards = visual.update(|_, cx| values(&snapshot(&panel, cx)));
        tap(visual, *cards.last().unwrap());
        click(visual, "ddz-play");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("ddz-status-thinking")
        });
        let cards = visual.update(|_, cx| values(&snapshot(&panel, cx)));
        let pair = cards
            .windows(2)
            .find(|cards| cards[0] < 52 && cards[0] / 4 == cards[1] / 4)
            .unwrap();
        let single = *cards.iter().find(|card| **card / 4 != pair[0] / 4).unwrap();
        let selected = |visual: &mut VisualTestContext| {
            visual.update(|_, cx| snapshot(&panel, cx).matches("ddz-selected-").count())
        };
        tap(visual, single);
        assert_eq!(selected(visual), 1);
        tap(visual, single);
        assert_eq!(selected(visual), 0);
        tap(visual, single);
        drag(visual, pair[0], pair[1], false);
        assert_eq!(selected(visual), 2);
        let tree = visual.update(|_, cx| snapshot(&panel, cx));
        let chosen: Vec<u8> = tree
            .split("ddz-selected-")
            .skip(1)
            .map(|tail| {
                tail.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .unwrap()
            })
            .collect();
        assert_eq!(
            chosen, pair,
            "hand: {cards:?}, previous selection: {single}"
        );
        assert!(!tree.contains("ddz-controls"));
        drag(visual, pair[1], pair[0], false);
        assert_eq!(selected(visual), 0);
        drag(visual, pair[1], pair[0], true);
        assert_eq!(selected(visual), 2);
        let hover = position(visual, single);
        visual.simulate_mouse_move(hover, None, Modifiers::default());
        assert_eq!(selected(visual), 2);
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("ddz-status-turn")
        });
        assert_eq!(selected(visual), 2);
        tap(visual, pair[0]);
        assert_eq!(selected(visual), 1);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
