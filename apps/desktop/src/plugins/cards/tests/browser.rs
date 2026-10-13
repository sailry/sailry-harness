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

#[test]
fn follows_declared_sign_in_redirects() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../../plugins/notion/plugin.json")).unwrap();
    let target: card::Target = serde_json::from_value(
        manifest["extensions"]["dev.sailry.platform"]["cards"][0]["capabilities"]["browser"][0]
            .clone(),
    )
    .unwrap();
    for url in [
        "https://mcp.notion.com/authorize",
        "https://www.notion.so/login",
        "https://notion.so/login",
        "https://app.notion.com/login",
    ] {
        assert!(super::super::browser::allowed(url, &target, None));
    }
    for url in [
        "https://app.notion.com.outside.test/login",
        "https://outside.test/login",
        "http://app.notion.com/login",
    ] {
        assert!(!super::super::browser::allowed(url, &target, None));
    }
}
