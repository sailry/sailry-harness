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
