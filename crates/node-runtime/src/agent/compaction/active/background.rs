//! One prepared immutable prefix; only the existing callback persists it.
use super::*;

pub(super) struct Candidate {
    prefix: Vec<String>,
    capture: Content,
    tools: std::collections::HashMap<String, serde_json::Value>,
    masks: model::history::Masks,
    stop: CancellationToken,
    task: Option<tokio::task::JoinHandle<adk_core::Result<Option<Event>>>>,
}

impl Candidate {
    pub(super) fn start(events: Vec<Event>, capture: Content, mut summarizer: Summarizer) -> Self {
        let stop = summarizer.stop.child_token();
        summarizer.stop = stop.clone();
        let tools = summarizer.tools.clone().unwrap_or_default();
        let masks = summarizer.masks.clone().unwrap_or_default();
        let prefix = events.iter().map(|event| event.id.clone()).collect();
        let task = tokio::spawn(async move {
            summarizer
                .summarize_with(&events, SummaryMode::Automatic)
                .await
        });
        Self {
            prefix,
            capture,
            tools,
            masks,
            stop,
            task: Some(task),
        }
    }

    pub(super) fn matches(
        &self,
        history: &History,
        capture: &Content,
        tools: &std::collections::HashMap<String, serde_json::Value>,
        masks: &model::history::Masks,
    ) -> bool {
        self.masks == *masks
            && self.tools == *tools
            && self.capture.role == capture.role
            && self.capture.parts == capture.parts
            && history.events.len() >= self.prefix.len()
            && history
                .events
                .iter()
                .zip(&self.prefix)
                .all(|(event, id)| &event.id == id)
    }

    pub(super) fn ready(&self) -> bool {
        self.task
            .as_ref()
            .is_some_and(tokio::task::JoinHandle::is_finished)
    }

    pub(super) async fn finish(mut self) -> adk_core::Result<(usize, Option<Event>)> {
        let task = self.task.take().expect("prepared summary owns one task");
        let summary = task
            .await
            .map_err(|_| invalid("prepared context summary was interrupted"))??;
        Ok((self.prefix.len(), summary))
    }
}

impl Drop for Candidate {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_captured_prefix_and_appended_suffix() {
        let capture = Content::new("system").with_text("Captured instructions");
        let mut old = Event::new("old");
        old.id = "old".into();
        let mut appended = Event::new("current");
        appended.id = "appended".into();
        let mut history = History {
            events: vec![old, appended],
            completed: vec![1],
            current: 1,
            recent: None,
        };
        let candidate = Candidate {
            prefix: vec!["old".into()],
            capture: capture.clone(),
            tools: Default::default(),
            masks: Default::default(),
            stop: CancellationToken::new(),
            task: None,
        };
        assert!(candidate.matches(&history, &capture, &Default::default(), &Default::default()));
        assert!(!candidate.matches(
            &history,
            &Content::new("system").with_text("Updated instructions"),
            &Default::default(),
            &Default::default(),
        ));
        let changed = [("tool_search".into(), json!({"type":"function"}))].into();
        assert!(!candidate.matches(&history, &capture, &changed, &Default::default()));
        let masks = [("call".into(), "revision".into())].into();
        assert!(!candidate.matches(&history, &capture, &Default::default(), &masks));
        history.events[0].id = "replacement".into();
        assert!(!candidate.matches(&history, &capture, &Default::default(), &Default::default()));
    }
}
