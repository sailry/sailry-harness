use super::*;
use core::prelude::v1::test;

#[gpui::test]
fn stable_column_alignment(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    for execution in [false, true] {
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| Connections::new(window, cx));
            view.update(cx, |view, _| {
                let entries = [
                    (7, "android", Some(Duration::from_millis(9))),
                    (8, "ios", None),
                    (9, "android", Some(Duration::from_millis(1248))),
                ];
                for (index, platform, latency) in entries.into_iter().chain(execution.then_some((
                    6,
                    "macos",
                    Some(Duration::ZERO),
                ))) {
                    view.devices.push(Device {
                        id: NodeId([index; 32]),
                        local: index == 6,
                        known: KnownDevice {
                            name: Some(
                                "A long device name that must not shift the status columns".into(),
                            ),
                            execution,
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
        for index in [7, 8, 9].into_iter().chain(execution.then_some(6)) {
            let name = bounds(index, "name");
            let status = bounds(index, "status");
            let latency = bounds(index, "latency");
            let action = bounds(index, "action");
            assert!(name.size.width > px(0.));
            if execution {
                assert!(name.right() < latency.left());
                assert!(latency.right() < action.left());
                assert!(action.right() < status.left());
            } else {
                assert!(name.right() < status.left());
                assert!(status.right() < latency.left());
                assert!(latency.right() < action.left());
            }
            assert_eq!(status.size.width, px(STATUS_WIDTH));
            assert_eq!(latency.size.width, px(LATENCY_WIDTH));
            assert_eq!(action.size.width, px(ACTION_WIDTH));
            assert_eq!(status.left(), bounds(7, "status").left());
            assert_eq!(latency.left(), bounds(7, "latency").left());
            assert_eq!(action.left(), bounds(7, "action").left());
        }
    }
}

mod latency {
    use super::*;

    fn device(execution: bool, local: bool, latency: Option<Duration>) -> Device {
        Device {
            id: NodeId([7; 32]),
            local,
            known: KnownDevice {
                name: None,
                execution,
                platform: None,
            },
            latency,
        }
    }

    #[test]
    fn omits_missing_host_values() {
        for (local, latency) in [(true, Some(Duration::ZERO)), (false, None)] {
            assert_eq!(device(true, local, latency).latency_text(), None);
        }
    }

    #[test]
    fn preserves_measurements_and_controller_values() {
        for execution in [false, true] {
            for millis in [9, 1248] {
                assert_eq!(
                    device(execution, false, Some(Duration::from_millis(millis))).latency_text(),
                    Some(format!("{millis} ms")),
                );
            }
        }
        assert_eq!(
            device(false, false, None).latency_text().as_deref(),
            Some("—")
        );
    }
}
