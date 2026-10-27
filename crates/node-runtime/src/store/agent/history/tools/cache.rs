//! Cache serialized public details after checking their current canonical record.
use sailry_protocol::tool::detail::Reference;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

const ENTRIES: usize = 64;
const BYTES: usize = 64 * 1024 * 1024;

#[derive(Default)]
pub(in crate::store) struct Cache {
    entries: Mutex<VecDeque<Entry>>,
}

struct Entry {
    reference: Reference,
    source: u64,
    text: Arc<str>,
    dependency: Option<u64>,
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
        let entry = entries.remove(index)?;
        if entry.source != source {
            return None;
        }
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
    ) {
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|entry| entry.reference != reference);
        if text.len() > BYTES {
            return;
        }
        while entries.len() >= ENTRIES
            || entries.iter().map(|entry| entry.text.len()).sum::<usize>() + text.len() > BYTES
        {
            entries.pop_front();
        }
        entries.push_back(Entry {
            reference,
            source,
            text,
            dependency,
        });
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
    #[test]
    fn reuses_only_unchanged_sources() {
        let cache = Cache::default();
        let reference = reference(0);
        let source = 1;
        let text: Arc<str> = Arc::from("serialized detail");
        cache.insert(reference.clone(), source, text.clone(), None);
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
        cache.insert(first.clone(), source, Arc::from("first"), None);
        for index in 1..=ENTRIES {
            cache.insert(reference(index), source, Arc::from("detail"), None);
        }
        assert!(cache.get(&first, source).is_none());
        assert_eq!(cache.entries.lock().unwrap().len(), ENTRIES);
    }
}
