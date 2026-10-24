use super::*;

#[test]
fn retains_unigrams_and_adjacent_bigrams() {
    let keywords = index("向量数据库");
    for term in ["向", "量", "数", "据", "库", "向量", "量数", "数据", "据库"] {
        assert!(keywords.split_whitespace().any(|token| token == term));
    }
    assert_eq!(index("向").trim(), "向");
    assert!(index("ASCII café C++").is_empty());
}

#[test]
fn separates_all_supported_scripts() {
    for text in ["漢字", "すし", "カナ", "한글", "𠀀𠀁"] {
        let characters = text.chars().collect::<Vec<_>>();
        let keywords = index(text);
        assert!(keywords.contains(&field(&characters[0].to_string())));
        assert!(
            keywords
                .split_whitespace()
                .any(|token| token == field(text))
        );
    }
    assert!(!cjk('・'));
    assert!(!cjk('。'));
}

#[test]
fn preserves_mixed_phrases_and_literals() {
    assert_eq!(phrase("AI向量"), "\"ai 向 量\"");
    assert_eq!(
        phrase("title:literal OR secret*"),
        "\"title literal or secret\""
    );
    assert_eq!(phrase("\"quoted\""), "\"quoted\"");
    assert_eq!(
        phrase("missing\" OR \"secretive"),
        "\"missing or secretive\""
    );
    assert!(phrase("\" * : ()").is_empty());
}

#[test]
fn normalizes_only_derived_fields() {
    assert_eq!(field("Café C++ C# RUST"), "cafe c++ c# rust");
    assert_eq!(field("A, B\nC"), "a b c");
    assert_eq!(field("Cafe\u{301}"), "cafe");
    assert_eq!(query("Cafe\u{301}"), "\"cafe\"");
    assert_eq!(index("AI向量"), "ai 向 量 向量");
}

#[test]
fn separates_literal_words_and_cjk_bigrams() {
    assert_eq!(
        query("rust async cancellation"),
        "\"async\" OR \"cancellation\" OR \"rust\""
    );
    assert_eq!(
        query("AI向量数据库"),
        "\"ai\" OR \"向量\" OR \"据库\" OR \"数据\" OR \"量数\""
    );
    assert_eq!(query("向"), "\"向\"");
    assert_eq!(query("rust rust"), "\"rust\"");
    assert_eq!(query("C++ C#"), "\"c#\" OR \"c++\"");
    assert!(query("\" ( ) * :").is_empty());
    assert_eq!(
        query("title:literal OR secret*"),
        "\"literal\" OR \"or\" OR \"secret\" OR \"title\""
    );
}
