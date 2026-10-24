use super::*;

async fn search(db: &Connection, term: &str) -> Vec<String> {
    read(
        db,
        "arbitrary-package",
        &Search {
            terms: vec![term.into()],
            ..query()
        },
    )
    .await
    .unwrap()
    .entries
    .into_iter()
    .map(|entry| entry.key)
    .collect()
}

#[tokio::test]
async fn matches_substrings_and_single_characters() {
    let db = database().await;
    let mut index = metadata();
    index.fields[0] = "我们使用向量数据库".into();
    insert(&db, "raw", &json!({"text":"original body"}), &index).await;
    for term in ["我们", "向量", "量数", "数据库", "库", "向"] {
        assert_eq!(search(&db, term).await, ["raw"], "{term}");
    }
    index.fields[0] = "向".into();
    insert(&db, "single", &json!(true), &index).await;
    assert!(search(&db, "向").await.contains(&"single".into()));
    for (key, title, term) in [
        ("kana", "すし屋", "し屋"),
        ("katakana", "カタカナ", "カナ"),
        ("hangul", "한국어", "국어"),
    ] {
        index.fields[0] = title.into();
        insert(&db, key, &json!(true), &index).await;
        assert_eq!(search(&db, term).await, [key]);
    }
    validate(&db).await.unwrap();
}

#[tokio::test]
async fn keeps_latin_terms_and_accent_folding() {
    let db = database().await;
    let mut index = metadata();
    for (key, title) in [
        ("mixed", "AI向量数据库"),
        ("plain", "向量数据库"),
        ("latin", "Café C++ C#"),
    ] {
        index.fields[0] = title.into();
        insert(&db, key, &json!(title), &index).await;
    }
    assert_eq!(search(&db, "AI向量").await, ["mixed"]);
    assert_eq!(search(&db, "AI").await, ["mixed"]);
    for term in ["CAFE", "c++", "c#"] {
        assert_eq!(search(&db, term).await, ["latin"]);
    }
    validate(&db).await.unwrap();
}

#[tokio::test]
async fn preserves_mixed_phrase_order() {
    let db = database().await;
    let mut index = metadata();
    for (key, title) in [("before", "AI 向量"), ("after", "向量 AI")] {
        index.fields[0] = title.into();
        insert(&db, key, &json!(true), &index).await;
    }
    for term in ["AI 向量", "AI向量"] {
        assert_eq!(search(&db, term).await, ["before"]);
    }
    assert_eq!(search(&db, "向量 AI").await, ["after"]);
}

#[tokio::test]
async fn retains_existing_memory_terms_and_weights() {
    let db = database().await;
    let index = |fields: [&str; 2], tag: &str, order| Index {
        fields: fields.map(str::to_owned),
        tags: vec![tag.into()],
        order,
        source: None,
    };
    insert(
        &db,
        "title",
        &json!(true),
        &index(["向 量 向量", ""], "active", 1),
    )
    .await;
    insert(
        &db,
        "body",
        &json!(true),
        &index(["", "向 量 向量"], "active", 2),
    )
    .await;
    insert(
        &db,
        "archived",
        &json!(true),
        &index(["向量", ""], "archived", 100),
    )
    .await;
    let mut input = Search {
        terms: vec!["向量".into()],
        all: vec!["active".into()],
        ..query()
    };
    let page = read(&db, "arbitrary-package", &input).await.unwrap();
    assert_eq!(
        page.entries
            .iter()
            .map(|entry| entry.key.as_str())
            .collect::<Vec<_>>(),
        ["title", "body"]
    );
    input.weights = [1, 3];
    assert_eq!(
        read(&db, "arbitrary-package", &input)
            .await
            .unwrap()
            .entries[0]
            .key,
        "body"
    );
    input.terms.clear();
    assert_eq!(
        read(&db, "arbitrary-package", &input)
            .await
            .unwrap()
            .entries[0]
            .key,
        "body"
    );
    input.any = vec!["missing".into()];
    assert!(
        read(&db, "arbitrary-package", &input)
            .await
            .unwrap()
            .entries
            .is_empty()
    );
}

#[tokio::test]
async fn treats_operators_as_literal_phrases() {
    let db = database().await;
    let mut index = metadata();
    index.fields[0] = "secretive".into();
    insert(&db, "other", &json!(true), &index).await;
    index.fields[0] = "向量".into();
    insert(&db, "cjk", &json!(true), &index).await;
    for term in [
        "missing\" OR \"secretive",
        "secret*",
        "field0:secretive",
        "向\" OR secretive",
    ] {
        assert!(search(&db, term).await.is_empty(), "{term}");
    }
    let page = read(
        &db,
        "arbitrary-package",
        &Search {
            terms: vec!["向量".into(), "secretive".into()],
            ..query()
        },
    )
    .await
    .unwrap();
    assert_eq!(page.entries.len(), 2);
}

#[tokio::test]
async fn normalization_preserves_original_values() {
    let db = database().await;
    let mut index = metadata();
    index.fields = ["AI向量数据库 Café C++".into(), "Original body".into()];
    let original = json!({"text":"AI向量数据库 Café C++", "nested":{"raw":"Café"}});
    insert(&db, "original", &original, &index).await;
    let page = read(
        &db,
        "arbitrary-package",
        &Search {
            terms: vec!["CAFE".into()],
            ..query()
        },
    )
    .await
    .unwrap();
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].value, original);
    let row = db
        .prepare("SELECT index_data FROM plugin_values WHERE name=?1 AND key=?2")
        .await
        .unwrap()
        .query_row(["arbitrary-package", "original"])
        .await
        .unwrap();
    assert_eq!(
        row.get::<Vec<u8>>(0).unwrap(),
        serde_json::to_vec(&index).unwrap()
    );
    validate(&db).await.unwrap();
}
