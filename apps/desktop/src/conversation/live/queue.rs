//! Queue editing drafts and command attempts; scheduling stays on the Node.
use super::*;
use crate::feedback::Recovery as _;
use sailry_protocol::{
    ErrorCode,
    conversation::{Pending, Queue},
};

mod actions;
mod render;

pub(super) struct Panel {
    owner: WeakEntity<View>,
    binding: Binding,
    images: Entity<attachments::images::Images>,
    session: Option<SessionId>,
    recipient: Option<TurnId>,
    child: bool,
    queue: Queue,
    connected: bool,
    guidance_blocked: bool,
    guidance_pending: Rc<Cell<bool>>,
    edit_pending: Rc<Cell<bool>>,
    editing: Option<Editing>,
    pending: bool,
    retry: Option<actions::Attempt>,
    error: Option<&'static str>,
    action: Option<Task<()>>,
    /// Input sent while a turn runs, shown here until the Node admits it.
    outgoing: Option<SharedString>,
    outgoing_attached: bool,
    /// Derived from the owning view's existing outgoing command identity.
    represented: Option<TurnId>,
}

struct Editing {
    turn: TurnId,
    revision: u64,
    input: Entity<TextareaState>,
    attachments: Vec<sailry_protocol::AttachmentId>,
    references: Vec<sailry_protocol::conversation::reference::Reference>,
}

impl Drop for Panel {
    fn drop(&mut self) {
        self.guidance_pending.set(false);
        self.edit_pending.set(false);
    }
}

impl Panel {
    fn guidance_pending(&self) -> bool {
        self.retry.as_ref().is_some_and(|attempt| {
            matches!(attempt.request.command, Command::GuideQueuedTurn { .. })
        })
    }

    fn sync_guidance(&self, cx: &mut Context<Self>) {
        let pending = self.guidance_pending();
        if self.guidance_pending.replace(pending) != pending {
            let _ = self.owner.update(cx, |_, cx| cx.notify());
        }
    }

    fn sync_editing(&self, window: &mut Window, cx: &mut Context<Self>) {
        let pending = self.retry.as_ref().is_some_and(|attempt| attempt.extract);
        let was_pending = self.edit_pending.replace(pending);
        if !pending && !was_pending {
            return;
        }
        _ = self.owner.update(cx, |view, cx| {
            view.input.update(cx, |input, cx| {
                input.set_readonly(pending || view.readonly(), cx)
            });
            if !pending {
                view.input.update(cx, |input, cx| input.focus(window, cx));
            }
            cx.notify();
        });
    }

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
        guidance_pending: Rc<Cell<bool>>,
        edit_pending: Rc<Cell<bool>>,
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
                    return notice.into();
                }
                let owner = cx.weak_entity();
                notice.scoped_action(move |_, _, cx| {
                    let owner = owner.clone();
                    crate::feedback::retry("queue-retry")
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
            guidance_blocked: false,
            guidance_pending,
            edit_pending,
            editing: None,
            pending: false,
            retry: None,
            error: None,
            action: None,
            outgoing: None,
            outgoing_attached: false,
            represented: None,
        }
    }

    pub(super) fn sync(&mut self, view: &View, cx: &mut Context<Self>) {
        let queue = view
            .history
            .snapshot
            .as_ref()
            .map(|snapshot| &snapshot.page.queue);
        let connected = view.connected();
        let guidance_blocked = view.busy() || view.outgoing.is_some();
        let recipient = view.recipient().map(|run| run.turn);
        let outgoing = view.queued_outgoing();
        let represented = view.transcript_pending().map(|item| item.turn);
        let outgoing_attached = outgoing.is_some()
            && view
                .outgoing
                .as_ref()
                .is_some_and(|message| !message.input.attachments.is_empty());
        if self.session != view.session()
            || self.recipient != recipient
            || self.child != view.readonly()
            || self.connected != connected
            || self.guidance_blocked != guidance_blocked
            || self.outgoing != outgoing
            || self.represented != represented
            || self.outgoing_attached != outgoing_attached
            || queue.is_some_and(|queue| *queue != self.queue)
        {
            self.outgoing = outgoing;
            self.represented = represented;
            self.outgoing_attached = outgoing_attached;
            self.session = view.session();
            self.recipient = recipient;
            self.child = view.readonly();
            self.connected = connected;
            self.guidance_blocked = guidance_blocked;
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
