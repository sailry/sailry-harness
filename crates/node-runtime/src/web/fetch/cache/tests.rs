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
fn preserves_unicode_page_boundaries() {
    let text = "中文🙂".repeat(web::PAGE_CHARS);
    let document = document(text.clone());
    let mut joined = String::new();
    let mut offset = 0;
    loop {
        let page = document.page(offset).unwrap();
        assert_eq!(page.content.chars().count(), web::PAGE_CHARS);
        assert_eq!(page.truncated, page.next_offset.is_some());
        joined.push_str(&page.content);
        let Some(next) = page.next_offset else { break };
        offset = next;
    }
    assert_eq!(joined, text);
    assert_eq!(document.page(text.chars().count()).unwrap().content, "");
    assert_eq!(
        document.page(text.chars().count() + 1).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn bounds_entries_without_evicting_active_reads() {
    let mut cache = Cache::default();
    let active = cache.slot("active", 0).unwrap();
    for index in 0..web::CACHE_ENTRIES {
        drop(cache.slot(&index.to_string(), 0).unwrap());
    }
    assert_eq!(cache.entries.len(), web::CACHE_ENTRIES);
    assert!(Arc::ptr_eq(&active, &cache.slot("active", 1).unwrap()));
    assert!(cache.slot("0", 1).is_err());
}

#[test]
fn expires_after_fifteen_minutes() {
    let mut document = document("text".into());
    assert!(!document.expired());
    document.fetched -= Duration::from_secs(web::CACHE_SECS);
    assert!(document.expired());
}
