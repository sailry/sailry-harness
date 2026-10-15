use super::*;
use core::prelude::v1::test;

#[gpui::test]
fn preview_isolation(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::preferences::init(cx);
        let panel = cx.new(Panel::new);
        panel.update(cx, |panel, cx| {
            panel.refresh(cx);
            assert!(panel.state == State::Unavailable);
            assert!(panel.task.is_none());
        });
    });
}

#[gpui::test]
fn authorization_settings(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::preferences::init(cx);
        let panel = cx.new(Panel::new);
        // A rejected system request can leave the status undetermined when
        // the launching application's TCC identity prohibits microphone use.
        panel.update(cx, |panel, cx| {
            panel.finish_authorization(Status::NotDetermined, cx);
            assert_eq!(panel.permission, Status::NotDetermined);
        });
    });
    // Returning from a request does not open another OS flow automatically.
    assert!(cx.opened_url().is_none());
}

#[gpui::test]
fn granted_authorization(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::preferences::init(cx);
        let panel = cx.new(Panel::new);
        panel.update(cx, |panel, cx| {
            panel.finish_authorization(Status::Granted, cx);
            assert_eq!(panel.permission, Status::Granted);
        });
    });
    assert!(cx.opened_url().is_none());
}
