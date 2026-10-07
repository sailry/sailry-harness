use super::*;
use core::prelude::v1::test;

struct Harness {
    items: Vec<Item>,
    events: Entity<Events>,
}

impl Render for Harness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .children(self.items.iter().enumerate().map(|(index, item)| {
                crate::resources::rows::row(index, false, cx)
                    .h_7()
                    .px_2()
                    .child(content(
                        item,
                        "file",
                        self.events.clone(),
                        "fixture".into(),
                        window,
                        cx,
                    ))
            }))
    }
}

#[test]
fn preserves_intrinsic_defaults() {
    let decoration: Decoration = serde_json::from_value(json!({
        "badges": [{"text": "+3", "tone": "success"}]
    }))
    .unwrap();
    assert_eq!(decoration.badges_width, None);
}

#[gpui::test]
fn keeps_right_columns_visible(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
    let events = cx.new(|_| Events::new(sender, CancellationToken::new()));
    let items = [
        vec![("+237", "success"), ("−164", "danger")],
        vec![("+237", "success")],
        vec![("−164", "danger")],
        vec![],
    ]
    .into_iter()
    .enumerate()
    .map(|(index, badges)| {
        serde_json::from_value(json!({
            "id": index.to_string(),
            "label": format!("{}.rs", "长文件名".repeat(24)),
            "decoration": {
                "detail": "nested/long/path/".repeat(16),
                "badges_width": 96,
                "badges": badges.into_iter().map(|(text,tone)| json!({"text":text,"tone":tone})).collect::<Vec<_>>(),
                "check": {"id": format!("fixture-check-{index}"), "label": "Stage file", "checked": false}
            }
        }))
        .unwrap()
    })
    .collect();
    let (_, visual) = cx.add_window_view(|window, cx| {
        let harness = cx.new(|_| Harness { items, events });
        Root::new(harness, window, cx)
    });
    let handle = visual.update(|window, _| window.window_handle());
    let mut label_widths = Vec::new();
    for width in [260., 480.] {
        visual.simulate_window_resize(handle, size(px(width), px(400.)));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let mut columns = Vec::new();
        for index in 0..4 {
            let bounds = |visual: &mut VisualTestContext, prefix: &str| {
                let selector = Box::leak(format!("{prefix}-{index}").into_boxed_str());
                visual.debug_bounds(selector).unwrap()
            };
            let label = bounds(visual, "resource-label");
            let detail = bounds(visual, "resource-detail");
            let badges = bounds(visual, "resource-badges");
            let check = bounds(visual, "fixture-check");
            assert_eq!(badges.size.width, px(96.));
            assert!(label.right() <= badges.left() && detail.right() <= badges.left());
            assert!(badges.right() < check.left() && check.right() <= px(width));
            for badge in 0..if index == 0 {
                2
            } else {
                usize::from(index < 3)
            } {
                let selector =
                    Box::leak(format!("resource-badge-{index}-{badge}").into_boxed_str());
                let badge = visual.debug_bounds(selector).unwrap();
                assert!(badge.left() >= badges.left() && badge.right() <= badges.right());
            }
            columns.push((badges.left(), badges.right(), check.left(), check.right()));
            if index == 0 {
                label_widths.push(label.size.width);
            }
        }
        assert!(columns.windows(2).all(|pair| pair[0] == pair[1]));
    }
    assert!(label_widths[0] < label_widths[1]);
    let check = visual.debug_bounds("fixture-check-0").unwrap();
    visual.simulate_click(check.center(), Modifiers::default());
    assert_eq!(
        receiver.try_recv().unwrap(),
        json!({"tree":"fixture", "id":"0", "kind":"check"})
    );
    assert!(receiver.try_recv().is_err());
}
