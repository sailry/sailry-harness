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
async fn matches_literal_search_terms() {
    let db = database().await;
    for scenario in [
        "matches_substrings_and_single_characters",
        "keeps_latin_terms_and_accent_folding",
        "preserves_mixed_phrase_order",
        "retains_existing_memory_terms_and_weights",
        "treats_operators_as_literal_phrases",
        "normalization_preserves_original_values",
    ] {
        let corpus = table("arbitrary-package");
        db.execute_batch(&format!(
            "DROP TABLE IF EXISTS {corpus}; DELETE FROM plugin_values;"
        ))
        .await
        .unwrap();
        eprintln!("scenario: {scenario}");
        match scenario {
            "matches_substrings_and_single_characters" => {
                Box::pin(async {
                    let mut index = metadata();
                    index.fields[0] = "我们使用向量数据库".into();
                    insert(&db, "raw", &json!({"text":"original body"}), &index).await;
                    for term in ["我们", "向量", "量数", "数据库", "库", "向"] {
                        assert_eq!(search(&db, term).await, ["raw"], "{scenario}: {term}");
                    }
                    index.fields[0] = "向".into();
                    insert(&db, "single", &json!(true), &index).await;
                    assert!(
                        search(&db, "向").await.contains(&"single".into()),
                        "{scenario}"
                    );
                    for (key, title, term) in [
                        ("kana", "すし屋", "し屋"),
                        ("katakana", "カタカナ", "カナ"),
                        ("hangul", "한국어", "국어"),
                    ] {
                        index.fields[0] = title.into();
                        insert(&db, key, &json!(true), &index).await;
                        assert_eq!(search(&db, term).await, [key], "{scenario}");
                    }
                    validate(&db).await.unwrap();
                })
                .await
            }
            "keeps_latin_terms_and_accent_folding" => {
                Box::pin(async {
                    let mut index = metadata();
                    for (key, title) in [
                        ("mixed", "AI向量数据库"),
                        ("plain", "向量数据库"),
                        ("latin", "Café C++ C#"),
                    ] {
                        index.fields[0] = title.into();
                        insert(&db, key, &json!(title), &index).await;
                    }
                    assert_eq!(search(&db, "AI向量").await, ["mixed"], "{scenario}");
                    assert_eq!(search(&db, "AI").await, ["mixed"], "{scenario}");
                    for term in ["CAFE", "c++", "c#"] {
                        assert_eq!(search(&db, term).await, ["latin"], "{scenario}");
                    }
                    validate(&db).await.unwrap();
                })
                .await
            }
            "preserves_mixed_phrase_order" => {
                Box::pin(async {
                    let mut index = metadata();
                    for (key, title) in [("before", "AI 向量"), ("after", "向量 AI")] {
                        index.fields[0] = title.into();
                        insert(&db, key, &json!(true), &index).await;
                    }
                    for term in ["AI 向量", "AI向量"] {
                        assert_eq!(search(&db, term).await, ["before"], "{scenario}");
                    }
                    assert_eq!(search(&db, "向量 AI").await, ["after"], "{scenario}");
                })
                .await
            }
            "retains_existing_memory_terms_and_weights" => {
                Box::pin(async {
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
                        ["title", "body"],
                        "{scenario}"
                    );
                    input.weights = [1, 3];
                    assert_eq!(
                        read(&db, "arbitrary-package", &input)
                            .await
                            .unwrap()
                            .entries[0]
                            .key,
                        "body",
                        "{scenario}"
                    );
                    input.terms.clear();
                    assert_eq!(
                        read(&db, "arbitrary-package", &input)
                            .await
                            .unwrap()
                            .entries[0]
                            .key,
                        "body",
                        "{scenario}"
                    );
                    input.any = vec!["missing".into()];
                    assert!(
                        read(&db, "arbitrary-package", &input)
                            .await
                            .unwrap()
                            .entries
                            .is_empty(),
                        "{scenario}"
                    );
                })
                .await
            }
            "treats_operators_as_literal_phrases" => {
                Box::pin(async {
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
                        assert!(search(&db, term).await.is_empty(), "{scenario}: {term}");
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
                    assert_eq!(page.entries.len(), 2, "{scenario}");
                })
                .await
            }
            "normalization_preserves_original_values" => {
                Box::pin(async {
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
                    assert_eq!(page.entries.len(), 1, "{scenario}");
                    assert_eq!(page.entries[0].value, original, "{scenario}");
                    let row = db
                        .prepare("SELECT index_data FROM plugin_values WHERE name=?1 AND key=?2")
                        .await
                        .unwrap()
                        .query_row(["arbitrary-package", "original"])
                        .await
                        .unwrap();
                    assert_eq!(
                        row.get::<Vec<u8>>(0).unwrap(),
                        serde_json::to_vec(&index).unwrap(),
                        "{scenario}"
                    );
                    validate(&db).await.unwrap();
                })
                .await
            }
            _ => unreachable!(),
        }
    }
}
