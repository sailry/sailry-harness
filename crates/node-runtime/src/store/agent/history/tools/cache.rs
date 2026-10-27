//! Cache serialized public details after checking their current canonical record.
use sailry_protocol::{ErrorCode, Fault, tool::detail::Reference};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const ENTRIES: usize = 64;
const BYTES: usize = 64 * 1024 * 1024;
const ACTIVE_BYTES: usize = 4 * BYTES;
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
    active_until: Option<Instant>,
}

impl Cache {
    pub(super) fn get(
        &self,
        reference: &Reference,
        source: u64,
    ) -> Option<(Arc<str>, Option<u64>)> {
        let mut entries = self.entries.lock().unwrap();
        let index = entries
            .iter()
            .position(|entry| entry.reference == *reference)?;
        let mut entry = entries.remove(index)?;
        if entry.source != source {
            return None;
        }
        entry.active_until = Some(Instant::now() + PAGE_LEASE);
        let result = (entry.text.clone(), entry.dependency);
        entries.push_back(entry);
        Some(result)
    }

    pub(super) fn insert(
        &self,
        reference: Reference,
        source: u64,
        text: Arc<str>,
        dependency: Option<u64>,
    ) -> Result<(), Fault> {
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|entry| entry.reference != reference);
        if text.len() > BYTES {
            return Ok(());
        }
        Self::trim(&mut entries);
        while entries.iter().map(|entry| entry.text.len()).sum::<usize>() + text.len()
            > ACTIVE_BYTES + BYTES
        {
            let Some(index) = entries.iter().position(|entry| !active(entry)) else {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "tool detail paging capacity is busy",
                ));
            };
            entries.remove(index);
        }
        if entries
            .iter()
            .filter(|entry| active(entry))
            .map(|entry| entry.text.len())
            .sum::<usize>()
            + text.len()
            > ACTIVE_BYTES
        {
            return Err(Fault::new(
                ErrorCode::Busy,
                "tool detail paging capacity is busy",
            ));
        }
        entries.push_back(Entry {
            reference,
            source,
            text,
            dependency,
            active_until: Some(Instant::now() + PAGE_LEASE),
        });
        Ok(())
    }

    pub(super) fn complete(&self, reference: &Reference, more: bool) {
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries
            .iter_mut()
            .find(|entry| entry.reference == *reference)
        {
            entry.active_until = more.then(|| Instant::now() + PAGE_LEASE);
        }
        Self::trim(&mut entries);
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
    entry
        .active_until
        .is_some_and(|until| until > Instant::now())
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
    #[test]
    fn reuses_only_unchanged_sources() {
        let cache = Cache::default();
        let reference = reference(0);
        let source = 1;
        let text: Arc<str> = Arc::from("serialized detail");
        cache
            .insert(reference.clone(), source, text.clone(), None)
            .unwrap();
        cache.complete(&reference, false);
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
            .insert(first.clone(), source, Arc::from("first"), None)
            .unwrap();
        cache.complete(&first, false);
        for index in 1..=ENTRIES {
            let reference = reference(index);
            cache
                .insert(reference.clone(), source, Arc::from("detail"), None)
                .unwrap();
            cache.complete(&reference, false);
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
        cache.insert(first.clone(), 1, left.clone(), None).unwrap();
        cache
            .insert(second.clone(), 1, right.clone(), None)
            .unwrap();
        for _ in 0..640 {
            assert!(Arc::ptr_eq(&cache.get(&first, 1).unwrap().0, &left));
            cache.complete(&first, true);
            assert!(Arc::ptr_eq(&cache.get(&second, 1).unwrap().0, &right));
            cache.complete(&second, true);
        }
        cache.complete(&first, false);
        cache.complete(&second, false);
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
        for index in 0..6 {
            cache
                .insert(reference(index), 1, text.clone(), None)
                .unwrap();
        }
        assert_eq!(
            cache.insert(reference(6), 1, text, None).unwrap_err().code,
            ErrorCode::Busy
        );
        let mut entries = cache.entries.lock().unwrap();
        for entry in entries.iter_mut() {
            entry.active_until = Some(Instant::now() - PAGE_LEASE);
        }
        Cache::trim(&mut entries);
        assert!(entries.iter().map(|entry| entry.text.len()).sum::<usize>() <= BYTES);
    }
}
