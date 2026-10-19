//! Kit exposes topmost-only dismissal; retain the owning focus for stacked forms.
use gpui_kit::component::WindowExt as _;
use gpui_kit::*;

#[derive(Default)]
pub(crate) struct Owned {
    focus: Option<FocusHandle>,
    window: Option<WindowId>,
    closing: bool,
    listener: Option<Subscription>,
}

impl Owned {
    pub(crate) fn bind<T: 'static>(
        state: &mut T,
        get: fn(&mut T) -> &mut Self,
        window: &mut Window,
        cx: &mut Context<T>,
    ) {
        let focus = window.focused(cx);
        let owned = get(state);
        owned.focus = focus.clone();
        owned.window = Some(window.window_handle().window_id());
        owned.closing = false;
        owned.listener.take();
        let listener = focus.map(|focus| {
            cx.on_focus_in(&focus, window, move |_, window, cx| {
                cx.defer_in(window, move |state, window, cx| {
                    get(state).flush(window, cx)
                });
            })
        });
        get(state).listener = listener;
    }

    pub(crate) fn presented(&self, cx: &App) -> bool {
        self.window
            .is_some_and(|id| cx.windows().iter().any(|window| window.window_id() == id))
            && self.focus.is_some()
    }

    pub(crate) fn dismissed(&mut self) {
        self.listener.take();
        self.focus = None;
        self.window = None;
        self.closing = false;
    }

    pub(crate) fn focused(&self, window: &Window, cx: &App) -> bool {
        self.window == Some(window.window_handle().window_id())
            && self
                .focus
                .as_ref()
                .is_some_and(|focus| focus.contains_focused(window, cx))
    }

    pub(crate) fn close(&mut self, window: &mut Window, cx: &mut App) {
        self.closing = true;
        self.flush(window, cx);
    }

    fn flush(&mut self, window: &mut Window, cx: &mut App) {
        if self.closing && self.focused(window, cx) {
            self.dismissed();
            window.close_dialog(cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn dismissal_releases_listener() {
        let released = Rc::new(Cell::new(0));
        let observed = released.clone();
        let mut owner = Owned {
            listener: Some(Subscription::new(move || observed.set(observed.get() + 1))),
            ..Default::default()
        };
        owner.dismissed();
        assert_eq!(released.get(), 1);
        assert!(owner.listener.is_none());
        owner.dismissed();
        drop(owner);
        assert_eq!(released.get(), 1);
    }

    #[test]
    fn owner_releases_listener() {
        let released = Rc::new(Cell::new(false));
        let observed = released.clone();
        let owner = Owned {
            listener: Some(Subscription::new(move || observed.set(true))),
            ..Default::default()
        };
        assert!(!released.get());
        drop(owner);
        assert!(released.get());
    }
}
