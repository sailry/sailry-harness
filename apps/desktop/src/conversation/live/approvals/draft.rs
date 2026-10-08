use super::*;

pub(super) struct Draft {
    pub(super) input: Entity<TextareaState>,
    pub(super) editing: bool,
}

impl View {
    pub(super) fn begin_reason(
        &mut self,
        id: ApprovalId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.approval_target(id).is_none() {
            return;
        }
        let draft = self.approvals.drafts.entry(id).or_insert_with(|| {
            let input = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder(tr("approval_reason"))
                    .auto_grow(1, 3)
            });
            cx.observe(&input, |_, _, cx| cx.notify()).detach();
            Draft {
                input,
                editing: true,
            }
        });
        draft.editing = true;
        let input = draft.input.clone();
        window.defer(cx, move |window, cx| {
            input.update(cx, |input, cx| input.focus(window, cx));
        });
        cx.notify();
    }

    pub(super) fn cancel_reason(&mut self, id: ApprovalId, cx: &mut Context<Self>) {
        if self.approval_target(id).is_none() {
            return;
        }
        if let Some(draft) = self.approvals.drafts.get_mut(&id) {
            draft.editing = false;
        }
        cx.notify();
    }

    pub(super) fn submit_reason(
        &mut self,
        id: ApprovalId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self.approvals.drafts.get(&id).filter(|draft| draft.editing) else {
            return;
        };
        let text = draft.input.read(cx).value().to_string();
        self.resolve_approval(id, Decision::DenyWithReason(text), window, cx);
    }
}
