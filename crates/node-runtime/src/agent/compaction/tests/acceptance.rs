use super::*;

#[test]
fn advertises_configured_output_tokens() {
    let (mut summarizer, _) = summarizer(vec![], false);
    summarizer.context = Some(500_000);
    summarizer.output = Some(128_000);
    for large in [false, true] {
        let mut events = history();
        if large {
            events[1].set_content(Content::new("model").with_text("Evidence ".repeat(12_000)));
        }
        let prepared = summarizer.prepare(&events).unwrap();
        assert_eq!(
            prepared.request.config.unwrap().max_output_tokens,
            Some(8_192)
        );
        assert!(
            prepared.request.contents[0].parts[0]
                .text()
                .unwrap()
                .contains("Summary output budget: 8192 tokens")
        );
    }
}

#[tokio::test]
async fn accepts_short_complete_histories() {
    let (summarizer, model) = summarizer(vec![complete("A complete handoff")], false);
    let mut events = history();
    events[0].set_content(Content::new("user").with_text("Hello"));
    events[1].set_content(Content::new("model").with_text("How can I help?"));
    let original = transcript(&events).unwrap();
    assert!(original.len() < 1_024);
    let summary = summarizer.summarize_events(&events).await.unwrap().unwrap();
    let content = summary.actions.compaction.unwrap().compacted_content;
    assert!(
        content.parts[0]
            .text()
            .unwrap()
            .starts_with("A complete handoff")
    );
    assert_eq!(model.requests.lock().unwrap().len(), 1);
    assert_eq!(transcript(&events).unwrap(), original);
}

#[tokio::test]
async fn accepts_equal_and_larger_outputs() {
    let (initial, _) = summarizer(vec![], false);
    let events = history();
    let original = transcript(&events).unwrap();
    let appendix = initial.covered_instruction(&events).unwrap().appendix();
    for length in [
        original.len() - appendix.len(),
        original.len() + 1,
        SUMMARY_BYTES + 1,
    ] {
        let body = "x".repeat(length);
        let (summarizer, model) = summarizer(vec![complete(&body)], false);
        let summary = summarizer.summarize_events(&events).await.unwrap().unwrap();
        let content = summary.actions.compaction.unwrap().compacted_content;
        assert_eq!(
            content.parts[0].text().unwrap(),
            format!("{body}{appendix}")
        );
        assert_eq!(model.requests.lock().unwrap().len(), 1);
        assert_eq!(transcript(&events).unwrap(), original);
    }
}

#[tokio::test]
async fn accepts_complete_low_output_responses() {
    for output in [1, 16] {
        let (mut summarizer, model) = summarizer(vec![complete("OK")], false);
        summarizer.output = Some(output);
        let events = history();
        assert!(
            summarizer
                .summarize_events(&events)
                .await
                .unwrap()
                .is_some()
        );
        let requests = model.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].config.as_ref().unwrap().max_output_tokens,
            Some(output)
        );
    }
}

#[test]
fn prefers_estimated_fitting_boundaries() {
    let (summarizer, _) = summarizer(vec![], false);
    let mut events = history();
    events[1].set_content(Content::new("model").with_text("Evidence ".repeat(12_000)));
    assert!(!summarizer.fits(&events).unwrap());
    assert!(summarizer.fits(&events[..1]).unwrap());
    assert_eq!(
        summarizer.prefix(&events, [2, 1].into_iter()).unwrap(),
        Some(1)
    );
}
