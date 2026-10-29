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

    pub(crate) fn compact_context(
        &mut self,
        focus: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_compact() {
            return;
        }
        let text = self.input.read(cx).value().to_string();
        let source = references::commands::leading(&text)
            .is_some_and(|(name, _, _)| name == "compact")
            .then_some(text);
        let session = self.session.as_ref().expect("session was checked");
        self.execute(
            Command::CompactContext {
                session: session.id,
                expected_revision: session.revision,
                focus,
            },
            actions::Action::Compact(source),
            window,
            cx,
        );
        self.focus(window, cx);
    }
}

pub(super) fn focus(text: &str) -> Option<String> {
    let (name, arguments, _) = references::commands::leading(text)?;
    (name == "compact" && !arguments.trim().is_empty()).then(|| arguments.trim().to_owned())
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
        Status::Continued => "turn_continued",
        Status::Interrupted => "chat_compact_interrupted",
        Status::Failed => "chat_compact_failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[core::prelude::v1::test]
    fn retains_manual_focus() {
        assert_eq!(
            focus(" /compact Keep pending questions\nand exact paths "),
            Some("Keep pending questions\nand exact paths".into())
        );
        assert_eq!(focus("/compact"), None);
        assert_eq!(focus("/compaction Keep exact paths"), None);
    }
}
