use super::*;

fn search(index: &Index, binding: &Binding, text: &str) -> Vec<String> {
    let mut input = query();
    input.query = text.into();
    input.mode = Mode::Keyword;
    index
        .search(
            &input,
            &[binding.clone()],
            &[],
            false,
            &CancellationToken::new(),
        )
        .unwrap()
        .hits
        .into_iter()
        .map(|hit| hit.citation.path)
        .collect()
}

#[test]
fn ranks_independent_words_by_relevance() {
    let (_directory, mut index) = open();
    let binding = binding();
    index.bind(&binding).unwrap();
    for (path, text) in [
        (
            "complete.md",
            "Rust handles cancellation of running async operations",
        ),
        ("partial.md", "Rust language syntax"),
        ("unrelated.md", "Database pool limits"),
    ] {
        index
            .replace(&binding, &document(path), &[part(text, None)])
            .unwrap();
    }
    for text in ["rust async cancellation", "cancellation rust async"] {
        assert_eq!(
            search(&index, &binding, text),
            ["complete.md", "partial.md"]
        );
    }
    assert!(search(&index, &binding, "\" * : ()").is_empty());
}

#[test]
fn matches_cjk_bigrams_without_phrase_order() {
    let (_directory, mut index) = open();
    let binding = binding();
    index.bind(&binding).unwrap();
    for (path, text) in [
        (
            "complete.md",
            "管理资料，使用关键词完成检索，也提供有效方法",
        ),
        ("partial.md", "资料归档"),
        ("unrelated.md", "資方交付料品，检查锁方与索引方位"),
    ] {
        index
            .replace(&binding, &document(path), &[part(text, None)])
            .unwrap();
    }
    for text in ["资料检索方法", "方法检索资料"] {
        assert_eq!(
            search(&index, &binding, text),
            ["complete.md", "partial.md"]
        );
    }
    assert_eq!(search(&index, &binding, "档"), ["partial.md"]);
}

#[test]
fn keeps_match_syntax_literal() {
    let (_directory, mut index) = open();
    let binding = binding();
    index.bind(&binding).unwrap();
    for (path, text) in [
        ("complete.md", "Rust C++ café language"),
        ("unrelated.md", "secretive"),
    ] {
        index
            .replace(&binding, &document(path), &[part(text, None)])
            .unwrap();
    }
    for text in ["RUST", "CAFE", "c++", "text:rust", "\"rust\" OR missing*"] {
        assert_eq!(search(&index, &binding, text), ["complete.md"]);
    }
    assert!(search(&index, &binding, "secret*").is_empty());
}
