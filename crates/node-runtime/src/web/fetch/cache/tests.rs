use super::*;

fn document(content: String) -> Document {
    Document {
        url: "https://example.com/".into(),
        status: 200,
        content_type: "text/plain".into(),
        title: None,
        content,
        fetched: Instant::now(),
    }
}

#[test]
fn retains_bounded_page_entries() {
    for (case_name, run) in [
        (
            "preserves_unicode_page_boundaries",
            (|case_name| {
                let text = "中文🙂".repeat(web::PAGE_CHARS);
                let document = document(text.clone());
                let mut joined = String::new();
                let mut offset = 0;
                loop {
                    let page = document.page(offset).unwrap();
                    assert_eq!(page.content.chars().count(), web::PAGE_CHARS, "{case_name}");
                    assert_eq!(page.truncated, page.next_offset.is_some(), "{case_name}");
                    joined.push_str(&page.content);
                    let Some(next) = page.next_offset else { break };
                    offset = next;
                }
                assert_eq!(joined, text, "{case_name}");
                assert_eq!(
                    document.page(text.chars().count()).unwrap().content,
                    "",
                    "{case_name}"
                );
                assert_eq!(
                    document.page(text.chars().count() + 1).unwrap_err().code,
                    ErrorCode::InvalidRequest,
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "bounds_entries_without_evicting_active_reads",
            (|case_name| {
                let mut cache = Cache::default();
                let active = cache.slot("active", 0).unwrap();
                for index in 0..web::CACHE_ENTRIES {
                    drop(cache.slot(&index.to_string(), 0).unwrap());
                }
                assert_eq!(cache.entries.len(), web::CACHE_ENTRIES, "{case_name}");
                assert!(
                    Arc::ptr_eq(&active, &cache.slot("active", 1).unwrap()),
                    "{case_name}"
                );
                assert!(cache.slot("0", 1).is_err(), "{case_name}");
            }) as fn(&str),
        ),
        (
            "expires_after_fifteen_minutes",
            (|case_name| {
                let mut document = document("text".into());
                assert!(!document.expired(), "{case_name}");
                document.fetched -= Duration::from_secs(web::CACHE_SECS);
                assert!(document.expired(), "{case_name}");
            }) as fn(&str),
        ),
        (
            "admits_active_urls_beyond_cache_retention",
            (|case_name| {
                let mut cache = Cache::default();
                let active: Vec<_> = (0..web::CACHE_ENTRIES + 2)
                    .map(|index| cache.slot(&index.to_string(), 0).unwrap())
                    .collect();
                for (index, slot) in active.iter().enumerate() {
                    assert!(
                        Arc::ptr_eq(slot, &cache.slot(&index.to_string(), 0).unwrap()),
                        "{case_name}"
                    );
                }
                assert_eq!(cache.entries.len(), active.len(), "{case_name}");
                drop(active);
                cache.trim();
                assert_eq!(cache.entries.len(), web::CACHE_ENTRIES, "{case_name}");
            }) as fn(&str),
        ),
        (
            "notes_sparse_html_on_the_first_page",
            (|case_name| {
                let mut sparse = document("Loading".into());
                sparse.content_type = "text/html".into();
                assert_eq!(
                    sparse.page(0).unwrap().notice.as_deref(),
                    Some(SPARSE),
                    "{case_name}"
                );
                assert!(sparse.page(1).unwrap().notice.is_none(), "{case_name}");
                let mut full = document("Readable text ".repeat(40));
                full.content_type = "text/html".into();
                assert!(full.page(0).unwrap().notice.is_none(), "{case_name}");
                assert!(
                    document("Short".into()).page(0).unwrap().notice.is_none(),
                    "{case_name}"
                );
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}
