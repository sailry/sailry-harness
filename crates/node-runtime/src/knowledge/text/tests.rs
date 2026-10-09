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
        assert!(keywords.contains(&characters[0].to_string()));
        assert!(keywords.split_whitespace().any(|token| token == text));
    }
    assert!(!cjk('・'));
    assert!(!cjk('。'));
}

#[test]
fn preserves_mixed_phrases_and_literals() {
    let expression = query("AI向量");
    assert_eq!(expression, "\"AI 向  量 \"");
    assert_eq!(
        query("title:literal OR secret*"),
        "\"title:literal OR secret*\""
    );
    assert_eq!(query("\"quoted\""), "\"\"\"quoted\"\"\"");
    assert!(query("向\" OR secret").contains("\"\" OR secret"));
}
