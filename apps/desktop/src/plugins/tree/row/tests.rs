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
                let events = self.events.clone();
                let path = item.id.clone();
                crate::resources::rows::row(index, false, cx)
                    .h_7()
                    .px_2()
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        events
                            .read(cx)
                            .send(json!({"tree":"fixture", "id":path,"kind":"select"}));
                    })
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
    let badge_sets = [
        vec![("+237", "success"), ("−164", "danger")],
        vec![("+237", "success")],
        vec![("−164", "danger")],
        vec![],
    ];
    let labels = [
        "Changed".to_owned(),
        "src".to_owned(),
        format!("{}.rs", "长文件名".repeat(24)),
        format!("{}.rs", "长文件名".repeat(24)),
    ];
    let items = (0..8)
    .map(|index| {
        serde_json::from_value(json!({
            "id": index.to_string(),
            "label": labels[index % 4],
            "decoration": {
                "detail": (index >= 4).then(|| "nested/long/path/".repeat(16)),
                "badges_width": 96,
                "badges": badge_sets[index % 4].iter().map(|(text,tone)| json!({"text":text,"tone":tone})).collect::<Vec<_>>(),
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
    for width in [260., 320., 480.] {
        visual.simulate_window_resize(handle, size(px(width), px(400.)));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let mut columns = Vec::new();
        for index in 0..8 {
            let bounds = |visual: &mut VisualTestContext, prefix: &str| {
                let selector = Box::leak(format!("{prefix}-{index}").into_boxed_str());
                visual.debug_bounds(selector).unwrap()
            };
            let text = bounds(visual, "resource-text");
            let label = bounds(visual, "resource-label");
            let badges = bounds(visual, "resource-badges");
            let check = bounds(visual, "fixture-check");
            assert_eq!(label.left(), text.left());
            if index >= 4 {
                let detail = bounds(visual, "resource-detail");
                assert_eq!(detail.right(), text.right());
                assert!(label.right() <= detail.left());
                assert!(label.size.width >= detail.size.width);
            } else {
                assert_eq!(label.right(), text.right());
            }
            assert_eq!(badges.size.width, px(96.));
            assert!(text.right() < badges.left());
            assert!(badges.right() < check.left() && check.right() <= px(width));
            for badge in 0..badge_sets[index % 4].len() {
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
    assert!(label_widths.windows(2).all(|pair| pair[0] < pair[1]));
    for index in 0..8 {
        crate::plugins::tests::click(
            visual,
            Box::leak(format!("fixture-check-{index}").into_boxed_str()),
        );
        assert_eq!(
            receiver.try_recv().unwrap(),
            json!({"tree":"fixture", "id":index.to_string(), "kind":"check"})
        );
        assert!(receiver.try_recv().is_err());
        crate::plugins::tests::click(
            visual,
            Box::leak(format!("resource-label-{index}").into_boxed_str()),
        );
        assert_eq!(
            receiver.try_recv().unwrap(),
            json!({"tree":"fixture", "id":index.to_string(), "kind":"select"})
        );
        assert!(receiver.try_recv().is_err());
    }
}
