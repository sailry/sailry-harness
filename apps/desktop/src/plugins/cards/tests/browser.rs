use super::*;

#[test]
fn confines_navigation_without_agent_access() {
    let target = card::Target {
        name: "login".into(),
        origins: vec!["https://example.test".into()],
        source: card::Source::McpAuthorization,
        modes: vec![card::Mode::Embedded, card::Mode::External],
    };
    assert!(super::super::browser::allowed(
        "https://example.test/login",
        &target,
        None
    ));
    for url in [
        "http://example.test/login",
        "https://outside.test/login",
        "https://user@example.test/login",
        "file:///tmp/private",
        "javascript:alert(1)",
        "http://127.0.0.1:43191/other",
    ] {
        assert!(
            !super::super::browser::allowed(url, &target, Some("http://127.0.0.1:43191/callback")),
            "{url}"
        );
    }
    assert!(super::super::browser::allowed(
        "http://127.0.0.1:43191/callback?state=private&code=private",
        &target,
        Some("http://127.0.0.1:43191/callback")
    ));
    assert!(!super::super::browser::allowed(
        "http://127.0.0.1:43192/callback",
        &target,
        Some("http://127.0.0.1:43191/callback")
    ));
}
