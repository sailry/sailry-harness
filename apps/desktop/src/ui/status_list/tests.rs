use super::*;
use core::prelude::v1::test;

struct Panel;

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Content {
            id: "status-panel".into(),
            empty: "No items".into(),
            items: (0..48)
                .map(|index| Row {
                    id: format!("status-row-{index}"),
                    text: format!("Item {index}: {}", "Preserve full text 中文 🙂 ".repeat(20)),
                    state: State::Paused,
                    details: true,
                    highlight: false,
                })
                .collect(),
            footer: vec![
                button::Button::new("status-action")
                    .label("Clear")
                    .into_any_element(),
            ],
        }
    }
}

#[gpui::test]
fn keeps_footer_outside_bounded_rows(cx: &mut TestAppContext) {
    crate::conversation::live::tests::fixture::init(cx);
    let (_, visual) = cx.add_window_view(|window, cx| {
        let panel = cx.new(|_| Panel);
        Root::new(panel, window, cx)
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let panel = visual.debug_bounds("status-panel").unwrap();
    let scroll = visual.debug_bounds("status-panel-scroll").unwrap();
    let footer = visual.debug_bounds("status-panel-footer").unwrap();
    let first = visual.debug_bounds("status-row-0").unwrap();
    let last = visual.debug_bounds("status-row-47").unwrap();
    assert_eq!(first.size.height, px(32.));
    assert_eq!(last.size.height, px(32.));
    assert!(last.bottom() > scroll.bottom());
    assert!(scroll.size.height <= px(320.));
    assert!(footer.top() >= scroll.bottom());
    assert!(footer.bottom() <= panel.bottom());
    visual.update(|window, cx| {
        window.focus_next(cx);
        assert!(window.focused(cx).is_some());
        window.draw(cx).clear(cx);
    });
    press(visual, "enter");
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("status-row-0-details").is_some());
    assert!(
        visual
            .debug_bounds("status-details-status-row-0-content")
            .is_some()
    );
    let expanded_footer = visual.debug_bounds("status-panel-footer").unwrap();
    assert_eq!(expanded_footer, footer);
    press(visual, "enter");
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("status-row-0-details").is_none());
    visual.update(|window, _| window.remove_window());
}

fn press(visual: &mut VisualTestContext, key: &str) {
    let keystroke = Keystroke::parse(key).unwrap();
    visual.simulate_event(KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    visual.simulate_event(KeyUpEvent { keystroke });
}
