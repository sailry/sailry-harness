use super::*;
use core::prelude::v1::test;

#[gpui::test]
fn replacement_and_release_preserve_other_owners(cx: &mut TestAppContext) {
    let (first, second, untouched) = cx.update(|cx| {
        gpui_kit::init(cx);
        let first = Bindings::new(cx);
        let second = Bindings::new(cx);
        let untouched = cx.key_bindings().borrow().bindings().len();
        first.update(cx, |state, cx| {
            state.replace(
                vec![KeyBinding::new(
                    "ctrl-alt-j",
                    crate::shell::Search,
                    Some(&state.context()),
                )],
                cx,
            )
        });
        second.update(cx, |state, cx| {
            state.replace(
                vec![KeyBinding::new(
                    "ctrl-alt-k",
                    crate::shell::ToggleSidebar,
                    Some(&state.context()),
                )],
                cx,
            )
        });
        assert_eq!(cx.key_bindings().borrow().bindings().len(), untouched + 2);
        first.update(cx, |state, cx| {
            state.replace(
                vec![KeyBinding::new(
                    "ctrl-alt-l",
                    crate::shell::Search,
                    Some(&state.context()),
                )],
                cx,
            )
        });
        assert_eq!(cx.key_bindings().borrow().bindings().len(), untouched + 2);
        (first, second, untouched)
    });
    drop(first);
    cx.update(|_| {});
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(cx.key_bindings().borrow().bindings().len(), untouched + 1);
        assert!(
            cx.key_bindings()
                .borrow()
                .bindings_for_action(&crate::shell::Search)
                .next()
                .is_none()
        );
        assert_eq!(
            cx.key_bindings()
                .borrow()
                .bindings_for_action(&crate::shell::ToggleSidebar)
                .count(),
            1
        );
    });
    drop(second);
    cx.update(|_| {});
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(cx.key_bindings().borrow().bindings().len(), untouched));
}
