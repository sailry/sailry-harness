use super::*;
use core::prelude::v1::test;

#[test]
fn plain_body_preserves_bounded_source() {
    let text = "## Literal **Markdown** 中文 🙂\n\nKeep e\u{301} intact";
    assert_eq!(
        plain_body(Some(&HostValue::Str(text.into())))
            .unwrap()
            .as_str(),
        text
    );
    let limit = "🙂".repeat(TEXT_LIMIT / 4);
    assert_eq!(
        plain_body(Some(&HostValue::Str(limit.clone())))
            .unwrap()
            .as_str(),
        limit
    );
    assert!(plain_body(Some(&HostValue::Str(format!("{limit}x")))).is_none());
    assert!(plain_body(None).is_none());
    assert!(plain_body(Some(&HostValue::Null)).is_none());
    assert!(plain_body(Some(&HostValue::Number(1.))).is_none());
}
