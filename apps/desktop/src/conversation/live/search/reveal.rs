use super::*;
use sailry_client::conversation::HistoryRequest;

impl View {
    pub(in crate::conversation::live) fn reveal_match(
        &mut self,
        target: Match,
        cx: &mut Context<Self>,
    ) {
        self.reveal = Some(target);
        if self.reveal_loaded(cx) {
            return;
        }
        self.request_target(cx);
    }

    pub(in crate::conversation::live) fn request_target(&mut self, cx: &mut Context<Self>) {
        let Some(target) = &self.reveal else {
            return;
        };
        if let Some(requests) = &self.older {
            let sequence = target
                .turn_sequence
                .try_into()
                .expect("validated search turn sequence");
            if requests
                .try_send(HistoryRequest::Through(sequence))
                .is_err()
            {
                self.error = Some("chat_history_failed");
            }
        }
        cx.notify();
    }

    pub(in crate::conversation::live) fn reveal_loaded(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(target) = &self.reveal else {
            return false;
        };
        let Some(index) = self.rows.iter().position(|turn| *turn == target.turn) else {
            return false;
        };
        if !self.history.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot
                .page
                .entries
                .iter()
                .any(|entry| entry.id == target.entry)
        }) {
            return false;
        }
        self.scroller.update(cx, |scroller, cx| {
            scroller.scroll_to_item(index, cx);
        });
        self.reveal = None;
        cx.notify();
        true
    }
}
