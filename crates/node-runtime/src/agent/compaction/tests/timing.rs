use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Progress {
    requests: AtomicUsize,
}

#[async_trait]
impl Llm for Progress {
    fn name(&self) -> &str {
        "progress-fixture"
    }

    async fn generate_content(
        &self,
        _: LlmRequest,
        stream: bool,
    ) -> adk_core::Result<LlmResponseStream> {
        assert!(stream);
        self.requests.fetch_add(1, Ordering::SeqCst);
        Ok(Box::pin(futures::stream::iter(0..4).then(
            |index| async move {
                tokio::time::sleep(Duration::from_secs(90)).await;
                Ok(if index == 3 {
                    complete("Completed summary")
                } else {
                    LlmResponse {
                        content: Some(Content::new("model").with_text("Summary progress")),
                        partial: true,
                        ..Default::default()
                    }
                })
            },
        )))
    }
}

#[tokio::test(start_paused = true)]
async fn allows_progress_without_a_total_deadline() {
    let model = Arc::new(Progress {
        requests: AtomicUsize::new(0),
    });
    let (mut summarizer, _) = summarizer(vec![], false);
    summarizer.model = model.clone();
    let started = tokio::time::Instant::now();
    let summary = summarizer
        .summarize_events(&history())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(started.elapsed(), Duration::from_secs(360));
    assert_eq!(model.requests.load(Ordering::SeqCst), 1);
    assert!(
        summary.actions.compaction.unwrap().compacted_content.parts[0]
            .text()
            .unwrap()
            .starts_with("Completed summary")
    );
}
