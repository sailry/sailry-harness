use super::*;

fn direct(content: &str) -> Document {
    Document {
        url: "https://example.com/source".into(),
        status: 200,
        content_type: "text/html".into(),
        title: Some("Direct page".into()),
        content: content.into(),
        fetched: Instant::now(),
    }
}

fn response(url: &str, text: &str) -> Value {
    json!({"results":[{"url":"https://example.com/source","final_url":url,"title":"Rendered page","text":text}],"errors":[]})
}

#[test]
fn limits_trigger_to_short_successful_html() {
    let mut document = direct("");
    assert!(needed(&document));
    document.content = " \n\t ".into();
    assert!(needed(&document));
    document.content = "字".repeat(MIN_CONTENT_CHARS - 1);
    assert!(needed(&document));
    document.content.push('字');
    assert!(!needed(&document));
    document.content = "short".into();
    for status in [199, 300, 400, 404, 500] {
        document.status = status;
        assert!(!needed(&document));
    }
    document.status = 200;
    for content_type in [
        "text/plain",
        "text/markdown",
        "application/json",
        "application/pdf",
    ] {
        document.content_type = content_type.into();
        assert!(!needed(&document));
    }
    document.content_type = "application/xhtml+xml".into();
    assert!(needed(&document));
}

#[test]
fn preserves_snapshot_fields_and_markdown() {
    let document = parse(
        &response(
            "https://docs.example.com/final#fragment",
            "# Page\n\nSource excerpt",
        ),
        &direct("short"),
    )
    .unwrap();
    assert_eq!(document.url, "https://docs.example.com/final");
    assert_eq!(document.title.as_deref(), Some("Rendered page"));
    assert_eq!(document.content_type, "text/markdown");
    assert_eq!(document.status, 200);
    assert_eq!(document.content, "# Page\n\nSource excerpt");
    assert!(document.page(0).unwrap().untrusted);
}

#[test]
fn rejects_private_or_credentialed_redirects() {
    for url in [
        "http://127.0.0.1/private",
        "http://[::1]/private",
        "http://localhost/private",
        "https://user:fixture-key@example.com/",
    ] {
        let error = parse(&response(url, "Rendered excerpt"), &direct("short"))
            .err()
            .expect("redirect must be rejected");
        assert!(!error.message.contains("fixture-key"));
    }
}

#[test]
fn bounds_content_and_validates_target() {
    let document = direct("short");
    assert!(
        parse(
            &response(
                "https://example.com/final",
                &"a".repeat(web::MAX_BODY_BYTES + 1)
            ),
            &document
        )
        .is_err()
    );
    assert!(parse(&response("https://example.com/final", " \n\t"), &document).is_err());
    let mut wrong = response("https://example.com/final", "Rendered excerpt");
    wrong["results"][0]["url"] = "https://other.example/".into();
    assert!(parse(&wrong, &document).is_err());
    wrong["results"][0]["url"] = document.url.clone().into();
    wrong["results"][0]["text"] = json!({"body":"fixture-key"});
    let error = parse(&wrong, &document)
        .err()
        .expect("non-text content must be rejected");
    assert!(!error.message.contains("fixture-key"));
}

#[test]
fn sanitizes_target_errors() {
    for code in [
        "timeout",
        "invalid_url",
        "invalid_redirect_url",
        "page_not_found",
        "empty_content",
        "fixture-key",
    ] {
        let error = parse(&json!({"results":[],"errors":[{"url":"https://example.com/source","error":code,"message":"fixture-key"}]}), &direct("short")).err().expect("target failure must be reported");
        assert!(!error.message.contains("fixture-key"));
        if matches!(code, "invalid_url" | "invalid_redirect_url") {
            assert_eq!(error.code, ErrorCode::PermissionDenied);
        }
    }
}
