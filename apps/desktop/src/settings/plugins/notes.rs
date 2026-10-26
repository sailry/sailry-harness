//! Exact-version release notes stay plain text and use the contract's locale fallback.
use super::*;
use sailry_protocol::plugin::catalog::ReleaseNotes;

fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.strip_prefix("- ").unwrap_or(line))
}

pub(super) fn view(notes: &ReleaseNotes) -> AnyElement {
    v_flex()
        .px_3()
        .py_2()
        .gap_2()
        .text_sm()
        .children(lines(notes.text(&rust_i18n::locale())).map(|line| div().child(line.to_owned())))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn preserves_plain_release_notes() {
        assert_eq!(
            lines("- Improved tools\n\n<script>alert(1)</script>\nhttps://example.org")
                .collect::<Vec<_>>(),
            [
                "Improved tools",
                "<script>alert(1)</script>",
                "https://example.org"
            ]
        );
    }
}
