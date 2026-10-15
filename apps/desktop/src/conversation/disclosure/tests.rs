use super::*;
use core::prelude::v1::test;

#[test]
fn preserves_full_wrapped_accessible_labels() {
    let label: SharedString =
        "Complete prompt with Unicode 中文 🙂\nPreserve the final line".into();
    let detail = Detail {
        wrap: true,
        state: Some("Awaiting answer".into()),
        ..Default::default()
    };
    assert_eq!(
        detail.accessible(&label),
        "Complete prompt with Unicode 中文 🙂\nPreserve the final line Awaiting answer"
    );
}

#[gpui::test]
fn colors_failure_text_semantically(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            Theme::change(mode, None, cx);
            let detail = Detail::mono("missing.txt").state(Some("Read failed".into()), true);
            assert_eq!(tone(&detail, cx), cx.theme().danger);
            assert_ne!(tone(&Detail::default(), cx), cx.theme().danger);
            assert_eq!(
                tone(
                    &Detail {
                        pending: true,
                        ..Default::default()
                    },
                    cx
                ),
                cx.theme().warning
            );
            assert_eq!(
                detail.accessible(&"Read".into()),
                "Read missing.txt Read failed"
            );
            assert_eq!(
                tone(
                    &Detail {
                        running: true,
                        ..detail
                    },
                    cx
                ),
                cx.theme().danger
            );
        }
    });
}
