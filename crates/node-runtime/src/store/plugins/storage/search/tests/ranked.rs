use super::*;

#[tokio::test]
async fn matches_filtered_pages_without_retaining_values() {
    let db = database().await;
    for (key, title, body, order, tag) in [
        ("title", "vector 向量", "body", 9, "project"),
        ("body", "title", "vector 向量", 1, "global"),
        ("excluded", "vector 向量", "vector 向量", 0, "foreign"),
        ("literal", "vector OR absent", "body", 2, "project"),
    ] {
        let index = Index {
            fields: [title.into(), body.into()],
            tags: vec![tag.into()],
            order,
            source: None,
        };
        insert(&db, key, &json!({"large":"x".repeat(240*1024)}), &index).await;
    }
    for terms in [
        vec![],
        vec!["vector".into()],
        vec!["向量".into()],
        vec!["vector OR absent".into()],
    ] {
        let query = Search {
            terms,
            any: vec!["project".into(), "global".into()],
            limit: 1,
            ..query()
        };
        let ranked = ranked(&db, "arbitrary-package", &query).await.unwrap();
        let mut keys = Vec::new();
        let mut offset = 0;
        loop {
            let page = read(
                &db,
                "arbitrary-package",
                &Search {
                    offset,
                    ..query.clone()
                },
            )
            .await
            .unwrap();
            keys.extend(
                page.entries
                    .into_iter()
                    .map(|entry| (entry.key, entry.revision)),
            );
            let Some(next) = page.next else { break };
            assert!(next > offset);
            offset = next;
        }
        assert_eq!(ranked, keys);
        assert!(ranked.iter().all(|(key, _)| key != "excluded"));
    }
    let complete = ranked(
        &db,
        "arbitrary-package",
        &Search {
            limit: 1,
            offset: 2,
            ..query()
        },
    )
    .await
    .unwrap();
    assert_eq!(complete.len(), 4);
}
