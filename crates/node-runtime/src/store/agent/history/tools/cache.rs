//! Cache serialized public details after checking their current canonical record.
use sailry_protocol::{
    ErrorCode, Fault,
    tool::detail::{DETAIL_PAGE_BYTES, Reference},
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const ENTRIES: usize = 64;
const BYTES: usize = 64 * 1024 * 1024;
const CAPACITY: usize = 2 * BYTES;
const PAGE_LEASE: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(in crate::store) struct Cache {
    entries: Mutex<VecDeque<Entry>>,
}

struct Entry {
    reference: Reference,
    source: u64,
    text: Arc<str>,
    dependency: Option<u64>,
    active_until: Instant,
}

impl Cache {
    pub(super) fn get(
        &self,
        reference: &Reference,
        source: u64,
    ) -> Option<(Arc<str>, Option<u64>)> {
        let mut entries = self.entries.lock().unwrap();
        Self::trim(&mut entries);
        Self::cached(&mut entries, reference, source)
    }

    pub(super) fn insert_with(
        &self,
        reference: Reference,
        source: u64,
        dependency: Option<u64>,
        serialize: impl FnOnce() -> Result<Arc<str>, Fault>,
    ) -> Result<Arc<str>, Fault> {
        {
            let mut entries = self.entries.lock().unwrap();
            Self::trim(&mut entries);
            if let Some((text, _)) = Self::cached(&mut entries, &reference, source) {
                return Ok(text);
            }
        }

        // The request owns this value until admission; other readers keep paging.
        let text = serialize()?;
        if text.len() > BYTES {
            // Preserve total_bytes so controllers can use their summary-only path.
            return Ok(text);
        }

        let mut entries = self.entries.lock().unwrap();
        Self::trim(&mut entries);
        if let Some((text, _)) = Self::cached(&mut entries, &reference, source) {
            return Ok(text);
        }
        while entries.iter().map(|entry| entry.text.len()).sum::<usize>() + text.len() > CAPACITY {
            let Some(index) = entries.iter().position(|entry| !active(entry)) else {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "tool detail paging capacity is busy",
                ));
            };
            entries.remove(index);
        }
        entries.push_back(Entry {
            reference,
            source,
            text: text.clone(),
            dependency,
            active_until: lease(text.len()),
        });
        Self::trim(&mut entries);
        Ok(text)
    }

    fn cached(
        entries: &mut VecDeque<Entry>,
        reference: &Reference,
        source: u64,
    ) -> Option<(Arc<str>, Option<u64>)> {
        let index = entries
            .iter()
            .position(|entry| entry.reference == *reference)?;
        let mut entry = entries.remove(index)?;
        if entry.source != source {
            return None;
        }
        entry.active_until = lease(entry.text.len());
        let result = (entry.text.clone(), entry.dependency);
        entries.push_back(entry);
        Some(result)
    }

    fn trim(entries: &mut VecDeque<Entry>) {
        while entries.iter().filter(|entry| !active(entry)).count() > ENTRIES
            || entries
                .iter()
                .filter(|entry| !active(entry))
                .map(|entry| entry.text.len())
                .sum::<usize>()
                > BYTES
        {
            let Some(index) = entries.iter().position(|entry| !active(entry)) else {
                break;
            };
            entries.remove(index);
        }
    }
}

fn active(entry: &Entry) -> bool {
    entry.active_until > Instant::now()
}

fn lease(bytes: usize) -> Instant {
    Instant::now()
        + if bytes > DETAIL_PAGE_BYTES {
            PAGE_LEASE
        } else {
            Duration::ZERO
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reference(index: usize) -> Reference {
        Reference {
            session: sailry_protocol::SessionId::new(),
            turn: sailry_protocol::TurnId::new(),
            entry: format!("entry-{index}"),
            index: 0,
            call: Some("call".into()),
            revision: "a".repeat(64),
        }
    }
    fn expire(cache: &Cache) {
        let mut entries = cache.entries.lock().unwrap();
        for entry in entries.iter_mut() {
            entry.active_until = Instant::now() - PAGE_LEASE;
        }
        Cache::trim(&mut entries);
    }
    #[test]
    fn reuses_only_unchanged_sources() {
        let cache = Cache::default();
        let reference = reference(0);
        let source = 1;
        let text: Arc<str> = Arc::from("serialized detail");
        cache
            .insert_with(reference.clone(), source, None, || Ok(text.clone()))
            .unwrap();
        assert!(Arc::ptr_eq(
            &cache
                .insert_with(reference.clone(), source, None, || {
                    panic!("a concurrent insertion reuses the serialized detail")
                })
                .unwrap(),
            &text
        ));
        assert!(Arc::ptr_eq(
            &cache.get(&reference, source).unwrap().0,
            &text
        ));
        assert!(cache.get(&reference, 2).is_none());
        assert!(cache.get(&reference, source).is_none());
    }
    #[test]
    fn evicts_the_oldest_detail() {
        let cache = Cache::default();
        let first = reference(0);
        let source = 1;
        cache
            .insert_with(first.clone(), source, None, || Ok(Arc::from("first")))
            .unwrap();
        for index in 1..=ENTRIES * 4 {
            let reference = reference(index);
            cache
                .insert_with(reference.clone(), source, None, || Ok(Arc::from("detail")))
                .unwrap();
            assert!(cache.get(&reference, source).is_some());
            let entries = cache.entries.lock().unwrap();
            assert!(entries.len() <= ENTRIES);
            assert!(entries.iter().all(|entry| !active(entry)));
            assert!(entries.iter().map(|entry| entry.text.len()).sum::<usize>() <= BYTES);
        }
        assert!(cache.get(&first, source).is_none());
        assert_eq!(cache.entries.lock().unwrap().len(), ENTRIES);
    }
    #[test]
    fn retains_simultaneous_large_pages() {
        let cache = Cache::default();
        let first = reference(0);
        let second = reference(1);
        let left: Arc<str> = Arc::from("a".repeat(40 * 1024 * 1024));
        let right: Arc<str> = Arc::from("b".repeat(40 * 1024 * 1024));
        cache
            .insert_with(first.clone(), 1, None, || Ok(left.clone()))
            .unwrap();
        cache
            .insert_with(second.clone(), 1, None, || Ok(right.clone()))
            .unwrap();
        for _ in 0..640 {
            assert!(Arc::ptr_eq(&cache.get(&first, 1).unwrap().0, &left));
            assert!(Arc::ptr_eq(&cache.get(&second, 1).unwrap().0, &right));
        }
        let last = super::super::read::slice(
            first.clone(),
            &cache.get(&first, 1).unwrap().0,
            left.len() - 64 * 1024,
            64 * 1024,
        )
        .unwrap();
        assert_eq!(last.next_offset, None);
        // One reader reaching EOF cannot end another reader's shared lease.
        cache
            .insert_with(reference(2), 1, None, || Ok(left.clone()))
            .unwrap();
        assert_eq!(
            cache
                .insert_with(reference(3), 1, None, || Ok(left.clone()))
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        assert!(Arc::ptr_eq(&cache.get(&first, 1).unwrap().0, &left));
        assert!(Arc::ptr_eq(&cache.get(&second, 1).unwrap().0, &right));
        assert!(
            cache
                .entries
                .lock()
                .unwrap()
                .iter()
                .map(|entry| entry.text.len())
                .sum::<usize>()
                <= CAPACITY
        );
        expire(&cache);
        assert!(
            cache
                .entries
                .lock()
                .unwrap()
                .iter()
                .map(|entry| entry.text.len())
                .sum::<usize>()
                <= BYTES
        );
    }

    #[test]
    fn releases_abandoned_page_leases() {
        let cache = Cache::default();
        let text: Arc<str> = Arc::from("x".repeat(40 * 1024 * 1024));
        for index in 0..3 {
            cache
                .insert_with(reference(index), 1, None, || Ok(text.clone()))
                .unwrap();
        }
        let serialized = std::cell::Cell::new(false);
        assert_eq!(
            cache
                .insert_with(reference(3), 1, None, || {
                    serialized.set(true);
                    Ok(text.clone())
                })
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        // Admission uses the actual value size, rather than reserving 64 MiB.
        assert!(serialized.get());
        expire(&cache);
        serialized.set(false);
        cache
            .insert_with(reference(3), 1, None, || {
                serialized.set(true);
                Ok(text)
            })
            .unwrap();
        assert!(serialized.get());
        assert!(
            cache
                .entries
                .lock()
                .unwrap()
                .iter()
                .map(|entry| entry.text.len())
                .sum::<usize>()
                <= CAPACITY
        );
    }

    #[test]
    fn admits_small_details_among_large_leases() {
        let cache = Cache::default();
        let text: Arc<str> = Arc::from("x".repeat(33 * 1024 * 1024));
        let large = [reference(0), reference(1)];
        for reference in &large {
            cache
                .insert_with(reference.clone(), 1, None, || Ok(text.clone()))
                .unwrap();
        }
        let small = reference(2);
        cache
            .insert_with(small.clone(), 1, None, || Ok(Arc::from("s".repeat(1024))))
            .unwrap();
        assert_eq!(cache.get(&small, 1).unwrap().0.len(), 1024);
        for reference in &large {
            assert!(Arc::ptr_eq(&cache.get(reference, 1).unwrap().0, &text));
        }
    }

    #[test]
    fn serialization_keeps_cached_reads_available() {
        let cache = Cache::default();
        let first = reference(0);
        cache
            .insert_with(first.clone(), 1, None, || Ok(Arc::from("first")))
            .unwrap();
        let second = reference(1);
        cache
            .insert_with(second.clone(), 1, None, || {
                assert!(cache.entries.try_lock().is_ok());
                assert_eq!(&*cache.get(&first, 1).unwrap().0, "first");
                // Another reader can finish the same insertion while serialization runs.
                cache.insert_with(second.clone(), 1, None, || Ok(Arc::from("concurrent")))?;
                Ok(Arc::from("unused"))
            })
            .unwrap();
        assert_eq!(&*cache.get(&second, 1).unwrap().0, "concurrent");
        assert_eq!(cache.entries.lock().unwrap().len(), 2);
    }

    #[test]
    fn oversized_details_remain_request_owned() {
        let cache = Cache::default();
        let retained: Arc<str> = Arc::from("x".repeat(40 * 1024 * 1024));
        for index in 0..3 {
            cache
                .insert_with(reference(index), 1, None, || Ok(retained.clone()))
                .unwrap();
        }
        let reference = reference(3);
        let oversized: Arc<str> = Arc::from("y".repeat(BYTES + 1));
        let text = cache
            .insert_with(reference.clone(), 1, None, || Ok(oversized.clone()))
            .unwrap();
        assert!(Arc::ptr_eq(&text, &oversized));
        let page =
            super::super::read::slice(reference.clone(), &text, 0, DETAIL_PAGE_BYTES).unwrap();
        assert_eq!(page.total_bytes, BYTES + 1);
        assert!(cache.get(&reference, 1).is_none());
        let entries = cache.entries.lock().unwrap();
        assert_eq!(entries.len(), 3);
        assert!(entries.iter().map(|entry| entry.text.len()).sum::<usize>() <= CAPACITY);
    }
}
