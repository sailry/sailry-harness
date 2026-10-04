use super::*;
use core::prelude::v1::test;
use gpui_kit::component::plot::Plot;

fn fixture(incoming: f64, outgoing: f64) -> AreaChart<(String, f64, f64), String, f64> {
    chart(
        "transfer-fixture",
        vec![
            ("12:00:00".into(), incoming, outgoing),
            ("12:00:01".into(), 0., 0.),
        ],
        (rgb(0x3388ff).into(), "Incoming".into()),
        (rgb(0x11bb88).into(), "Outgoing".into()),
    )
}

#[gpui::test]
fn shared_zero_centered_domain(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        for (incoming, outgoing) in [(100., 1.), (1., 100.), (1., 1.), (0., 0.)] {
            let chart = fixture(incoming, outgoing);
            assert_eq!(Plot::id(&chart), Some("transfer-fixture".into()));
            for height in [80., 160.] {
                let bounds = Bounds::new(point(px(0.), px(0.)), size(px(400.), px(height)));
                let active = chart
                    .tooltip_state(point(px(0.), px(height / 2.)), bounds, cx)
                    .unwrap();
                assert_eq!(active.index, 0);
                assert_eq!(active.dots.len(), 2);
                let upper = active.dots[0].y;
                let lower = active.dots[1].y;
                assert!(upper >= px(4.) && upper <= px(height / 2.));
                assert!(lower >= px(height / 2.) && lower <= px(height - 4.));
                if incoming == outgoing {
                    assert!(
                        (px(height / 2.) - upper - (lower - px(height / 2.))).abs() < px(0.001)
                    );
                }
                let idle = chart
                    .tooltip_state(point(px(400.), px(height / 2.)), bounds, cx)
                    .unwrap();
                assert_eq!(idle.index, 1);
                assert!(idle.dots.iter().all(|dot| dot.y == px(height / 2.)));
            }
        }
    });
}

#[gpui::test]
fn native_hover_and_positive_labels(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| Empty);
    visual.update(|window, cx| {
        let chart = fixture(0.0452880859375, 109.00603675842285);
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(400.), px(80.)));
        for cursor_y in [10., 70.] {
            let cursor = point(px(0.), px(cursor_y));
            let state = chart.tooltip_state(cursor, bounds, cx).unwrap();
            assert_eq!(state.index, 0);
            assert!(state.dots[0].y < px(40.));
            assert!(state.dots[1].y > px(40.));
            assert!(chart.tooltip(&state, cursor, bounds, window, cx).is_some());
        }
        assert_eq!(value_label(109.00603675842285), "109.01");
        assert_eq!(value_label(-0.0452880859375), "0.05");
    });
}
