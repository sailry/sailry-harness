//! Explicit context maintenance uses the shared durable command and run projection.
use super::*;
use sailry_protocol::conversation::{Page, Part, Run, RunKind};

impl View {
    pub(super) fn context_limit(&self) -> Option<u32> {
        self.config.as_ref().and_then(|config| {
            self.providers()
                .find(|provider| provider.id == config.provider)?
                .models
                .iter()
                .find(|model| model.id == config.model)
                .map(|model| model.context)
        })
    }

    pub(super) fn can_compact(&self) -> bool {
        !self.readonly()
            && !self.pending
            && self.retry.is_none()
            && self.connected()
            && self.configured()
            && self.session.is_some()
            && self.history.snapshot.as_ref().is_some_and(|snapshot| {
                !snapshot.page.runs.iter().any(|run| {
                    run.kind == RunKind::Compaction
                        && matches!(
                            run.status,
                            Status::Queued | Status::Running | Status::Stopping
                        )
                })
            })
    }

    pub(crate) fn compact_context(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_compact() {
            return;
        }
        let session = self.session.as_ref().expect("session was checked");
        self.execute(
            Command::CompactContext {
                session: session.id,
                expected_revision: session.revision,
            },
            actions::Action::Compact,
            window,
            cx,
        );
        self.focus(window, cx);
    }
}

pub(super) fn status(run: &Run, page: &Page) -> &'static str {
    match run.status {
        Status::Queued => "chat_compact_queued",
        Status::Running => "chat_compacting",
        Status::Stopping => "chat_stopping",
        Status::Completed => {
            if page.entries.iter().any(|entry| {
                entry.turn == run.turn
                    && entry
                        .parts
                        .iter()
                        .any(|part| matches!(part, Part::Compaction(_)))
            }) {
                "chat_compacted"
            } else {
                "chat_compact_unneeded"
            }
        }
        Status::Cancelled => "chat_compact_cancelled",
        Status::Interrupted => "chat_compact_interrupted",
        Status::Failed => "chat_compact_failed",
    }
}
