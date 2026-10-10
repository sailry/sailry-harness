//! Queue editing drafts and command attempts; scheduling stays on the Node.
use super::*;
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
    recipient: Option<TurnId>,
    child: bool,
    queue: Queue,
    connected: bool,
    editing: Option<Editing>,
    pending: bool,
    retry: Option<actions::Attempt>,
    error: Option<&'static str>,
    action: Option<Task<()>>,
    /// Input sent while a turn runs, shown here until the Node admits it.
    outgoing: Option<SharedString>,
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
        self.editing.is_some() && !self.child
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
                        .is_some_and(|editing| {
                            !panel.child && panel.changed(editing) && !panel.pending
                        })
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
                                _ = owner.update(cx, |panel, cx| panel.retry(window, cx));
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
            recipient: None,
            child: false,
            queue: Queue::default(),
            connected: false,
            editing: None,
            pending: false,
            retry: None,
            error: None,
            action: None,
            outgoing: None,
        }
    }

    pub(super) fn sync(&mut self, view: &View, cx: &mut Context<Self>) {
        let queue = view
            .history
            .snapshot
            .as_ref()
            .map(|snapshot| &snapshot.page.queue);
        let connected = view.connected();
        let recipient = view.recipient().map(|run| run.turn);
        let outgoing = view.queued_outgoing();
        if self.session != view.session()
            || self.recipient != recipient
            || self.child != view.readonly()
            || self.connected != connected
            || self.outgoing != outgoing
            || queue.is_some_and(|queue| *queue != self.queue)
        {
            self.outgoing = outgoing;
            self.session = view.session();
            self.recipient = recipient;
            self.child = view.readonly();
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

    fn allows(&self, command: &Command) -> bool {
        !self.child
            || matches!(
                command,
                Command::ReadQueuedTurn { .. } | Command::RemoveQueuedTurn { .. }
            )
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
        _ = self.owner.update(cx, |view, cx| {
            view.input.update(cx, |input, cx| input.focus(window, cx))
        });
    }
}
