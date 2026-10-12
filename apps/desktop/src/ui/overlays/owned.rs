//! Kit exposes topmost-only dismissal; retain the owning focus for stacked forms.
use gpui_kit::component::WindowExt as _;
use gpui_kit::*;

#[derive(Default)]
pub(crate) struct Owned {
    focus: Option<FocusHandle>,
    window: Option<WindowId>,
    closing: bool,
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
        if let Some(focus) = focus {
            cx.on_focus_in(&focus, window, move |_, window, cx| {
                cx.defer_in(window, move |state, window, cx| {
                    get(state).flush(window, cx)
                });
            })
            .detach();
        }
    }

    pub(crate) fn presented(&self, cx: &App) -> bool {
        self.window
            .is_some_and(|id| cx.windows().iter().any(|window| window.window_id() == id))
            && self.focus.is_some()
    }

    pub(crate) fn dismissed(&mut self) {
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
