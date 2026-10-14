//! Failed snapshot reads retain restored pane identity until an explicit retry.
use super::{Shell, Target};
use crate::tr;
use gpui_kit::{
    component::{button::Button, *},
    *,
};
mod load;
mod state;
pub(crate) use state::State;

pub(super) struct View {
    target: Target,
    owner: WeakEntity<Shell>,
    pending: bool,
    error: Option<sailry_protocol::Fault>,
}

impl View {
    pub fn new(target: Target, owner: WeakEntity<Shell>) -> Self {
        Self {
            target,
            owner,
            pending: true,
            error: None,
        }
    }

    pub fn failed(&mut self, error: sailry_protocol::Fault, cx: &mut Context<Self>) {
        self.pending = false;
        self.error = Some(error);
        cx.notify();
    }
}

impl Render for View {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.pending {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(spinner::Spinner::new().large())
                .into_any_element();
        }
        let target = self.target;
        crate::empty_state::panel(IconName::TriangleAlert, "pane_restore_failed", cx)
            .gap_4()
            .child(
                Button::new("pane-restore-retry")
                    .debug_selector(move || format!("pane-restore-retry-{target:?}"))
                    .outline()
                    .label(tr("chat_retry"))
                    .on_click(cx.listener(|view, _, window, cx| {
                        let owner = view.owner.clone();
                        let target = view.target;
                        window.defer(cx, move |window, cx| {
                            let _ = owner.update(cx, |shell, cx| {
                                shell.restore_pane(target, window, cx);
                            });
                        });
                    })),
            )
            .into_any_element()
    }
}
