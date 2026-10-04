use super::*;
use core::prelude::v1::test;

#[gpui::test]
fn stable_column_alignment(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Connections::new(window, cx));
        view.update(cx, |view, _| {
            for (index, platform, latency) in [
                (7, "android", Some(Duration::from_millis(9))),
                (8, "ios", None),
                (9, "android", Some(Duration::from_millis(1248))),
            ] {
                view.devices.push(Device {
                    id: NodeId([index; 32]),
                    local: false,
                    known: KnownDevice {
                        name: Some(
                            "A long device name that must not shift the status columns".into(),
                        ),
                        execution: false,
                        platform: Some(platform.into()),
                    },
                    latency,
                });
            }
        });
        Root::new(view, window, cx)
    });
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let mut bounds = |index, column| {
        let id = NodeId([index; 32]);
        visual
            .debug_bounds(Box::leak(
                format!("connection-{column}-{id:?}").into_boxed_str(),
            ))
            .unwrap()
    };
    for index in [7, 8, 9] {
        let name = bounds(index, "name");
        let status = bounds(index, "status");
        let latency = bounds(index, "latency");
        let action = bounds(index, "action");
        assert!(name.size.width > px(0.));
        assert!(name.right() < status.left());
        assert!(status.right() < latency.left());
        assert!(latency.right() < action.left());
        assert_eq!(status.size.width, px(STATUS_WIDTH));
        assert_eq!(latency.size.width, px(LATENCY_WIDTH));
        assert_eq!(action.size.width, px(ACTION_WIDTH));
        assert_eq!(status.left(), bounds(7, "status").left());
        assert_eq!(latency.left(), bounds(7, "latency").left());
        assert_eq!(action.left(), bounds(7, "action").left());
    }
}
