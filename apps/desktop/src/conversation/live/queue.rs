//! Queue editing drafts and command attempts; scheduling stays on the Node.
use super::*;
use gpui_kit::component::popover::PopoverState;
use sailry_protocol::{
    ErrorCode,
    conversation::{Pending, Queue},
};

mod actions;
mod render;
#[cfg(test)]
mod tests;

pub(super) struct Panel {
    owner: WeakEntity<View>,
    binding: Binding,
    images: Entity<attachments::images::Images>,
    session: Option<SessionId>,
    queue: Queue,
    connected: bool,
    editing: Option<Editing>,
    pending: bool,
    retry: Option<actions::Attempt>,
    error: Option<&'static str>,
    action: Option<Task<()>>,
    popover: Option<WeakEntity<PopoverState>>,
}

struct Editing {
    turn: TurnId,
    revision: u64,
    input: Entity<TextareaState>,
    attachments: Vec<sailry_protocol::AttachmentId>,
    references: Vec<sailry_protocol::conversation::reference::Reference>,
}

impl Panel {
    pub(super) fn update_pending(&self) -> bool {
        self.pending || self.retry.is_some()
    }
    pub(super) fn editing_draft(&self) -> bool {
        self.editing.is_some()
    }
    pub(super) fn new(
        owner: WeakEntity<View>,
        binding: Binding,
        images: Entity<attachments::images::Images>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Self {
        crate::feedback::observe_with(
            window,
            cx,
            |panel: &Self, _| {
                panel
                    .error
                    .or(panel
                        .editing
                        .as_ref()
                        .is_some_and(|editing| panel.changed(editing) && !panel.pending)
                        .then_some("queue_draft_changed"))
                    .into_iter()
                    .collect()
            },
            |panel, key, cx| {
                let notice = gpui_kit::component::notification::Notification::error(tr(key));
                if panel.retry.is_none() {
                    return notice;
                }
                let owner = cx.weak_entity();
                notice.action(move |_, _, cx| {
                    let owner = owner.clone();
                    gpui_kit::component::button::Button::new("queue-retry")
                        .label(tr("chat_retry"))
                        .debug_selector(|| "queue-retry".into())
                        .on_click(cx.listener(move |toast, _, window, cx| {
                            toast.dismiss(window, cx);
                            let owner = owner.clone();
                            window.defer(cx, move |window, cx| {
                                _ = owner.update(cx, |panel, cx| {
                                    if let Some(popover) =
                                        panel.popover.as_ref().and_then(WeakEntity::upgrade)
                                    {
                                        popover.update(cx, |popover, cx| popover.show(window, cx));
                                    }
                                    panel.retry(window, cx);
                                });
                            });
                        }))
                })
            },
        );
        Self {
            owner,
            binding,
            images,
            session: None,
            queue: Queue::default(),
            connected: false,
            editing: None,
            pending: false,
            retry: None,
            error: None,
            action: None,
            popover: None,
        }
    }

    pub(super) fn sync(&mut self, view: &View, cx: &mut Context<Self>) {
        let queue = view
            .history
            .snapshot
            .as_ref()
            .map(|snapshot| &snapshot.page.queue);
        let connected = view.connected();
        if self.session != view.session()
            || self.connected != connected
            || queue.is_some_and(|queue| *queue != self.queue)
        {
            self.session = view.session();
            self.connected = connected;
            if let Some(queue) = queue {
                self.queue = queue.clone();
            }
            cx.notify();
        }
    }

    fn blocked(&self) -> bool {
        !self.connected || self.pending || self.retry.is_some()
    }

    fn changed(&self, editing: &Editing) -> bool {
        !self
            .queue
            .items
            .iter()
            .any(|item| item.turn == editing.turn && item.revision == editing.revision)
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.retry.is_some() {
            return;
        }
        self.editing = None;
        self.error = None;
        self.restore_focus(window, cx);
        cx.notify();
    }

    fn restore_focus(&self, window: &mut Window, cx: &mut App) {
        if let Some(popover) = self.popover.as_ref().and_then(WeakEntity::upgrade)
            && popover.read(cx).is_open()
        {
            popover.read(cx).focus_handle(cx).focus(window, cx);
        }
    }
}
