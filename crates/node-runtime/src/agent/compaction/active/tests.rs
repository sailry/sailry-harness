use super::*;

#[test]
fn preserves_retained_provider_parts() {
    let preamble = Content::new("system").with_text("Instructions");
    let content = Content::new("model").with_text("Repeated response");
    let mut event = Event::new("invocation");
    let mut augmented = content.clone();
    augmented.parts.push(Part::Thinking {
        thinking: "Retained reasoning".into(),
        signature: Some("retained-signature".into()),
    });
    event.set_content(augmented.clone());
    let mut request = LlmRequest {
        model: "fixture".into(),
        contents: vec![
            preamble.clone().with_text("Scoped project instructions"),
            content,
            augmented.clone(),
        ],
        config: None,
        tools: Default::default(),
        previous_response_id: None,
    };
    let index = tail(&request, &[event.clone()], &preamble, "current").unwrap();
    assert_eq!(index, 2);
    request
        .contents
        .splice(1..index, [Content::new("user").with_text("Summary")]);
    assert_eq!(request.contents[2].parts, augmented.parts);
    assert!(
        tail(
            &request,
            &[event.clone()],
            &Content::new("system").with_text("Changed instructions"),
            "current",
        )
        .is_none()
    );
    request.contents[2].parts.reverse();
    assert!(tail(&request, &[event], &preamble, "current").is_none());
}
