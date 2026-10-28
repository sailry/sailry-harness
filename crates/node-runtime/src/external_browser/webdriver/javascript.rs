//! Adapted from ADK-Rust 675c02b6c0df6d632b953a06a5a12d1f814b7780.
//! Copyright 2026 Zavora Technologies Ltd.; Apache-2.0, see third_party_licenses/adk-LICENSE.
//! String escaping utilities for safe JavaScript interpolation.

/// Escape a string for safe interpolation into JavaScript code.
///
/// This prevents CSS selector injection attacks when user-supplied
/// selectors are interpolated into JavaScript strings. It escapes
/// characters that could break out of a JS string literal or inject
/// malicious code.
///
pub(crate) fn escape(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len() + 16);
    for ch in input.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '\'' => escaped.push_str("\\'"),
            '"' => escaped.push_str("\\\""),
            '`' => escaped.push_str("\\`"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\0' => escaped.push_str("\\0"),
            '<' => escaped.push_str("\\x3c"),
            '>' => escaped.push_str("\\x3e"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_quotes() {
        assert_eq!(escape("a'b"), "a\\'b");
    }

    #[test]
    fn double_quotes() {
        assert_eq!(escape("a\"b"), "a\\\"b");
    }

    #[test]
    fn backslash() {
        assert_eq!(escape("a\\b"), "a\\\\b");
    }

    #[test]
    fn backtick() {
        assert_eq!(escape("a`b"), "a\\`b");
    }

    #[test]
    fn newlines() {
        assert_eq!(escape("a\nb"), "a\\nb");
        assert_eq!(escape("a\rb"), "a\\rb");
    }

    #[test]
    fn null_byte() {
        assert_eq!(escape("a\0b"), "a\\0b");
    }

    #[test]
    fn script_tags() {
        assert_eq!(escape("</script>"), "\\x3c/script\\x3e");
    }

    #[test]
    fn normal_selector() {
        assert_eq!(escape("#my-button"), "#my-button");
        assert_eq!(escape(".nav-link"), ".nav-link");
        assert_eq!(escape("button[type=submit]"), "button[type=submit]");
    }

    #[test]
    fn injection_attempt() {
        let malicious = "'); document.cookie='stolen'; ('";
        let escaped = escape(malicious);
        // The single quotes are escaped, so the string can't break out of a JS literal
        assert!(escaped.starts_with("\\'"));
        assert!(escaped.contains("\\'"));
    }

    #[test]
    fn empty_string() {
        assert_eq!(escape(""), "");
    }
}
