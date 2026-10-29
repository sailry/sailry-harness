use sailry_protocol::search::{Page, Request, Selection};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

const LIFETIME: Duration = Duration::from_secs(5 * 60);
const CAPACITY: usize = 50;

#[derive(PartialEq, Eq)]
pub(super) struct Key {
    selection: Option<Selection>,
    input: Request,
}

impl Key {
    pub(super) fn new(selection: Option<Selection>, input: Request) -> Self {
        Self {
            selection,
            input: input.normalized(),
        }
    }
}

struct Entry {
    key: Key,
    page: Page,
    created: Instant,
}

#[derive(Default)]
pub(super) struct Cache {
    entries: VecDeque<Entry>,
}

impl Cache {
    pub(super) fn get(&mut self, key: &Key, now: Instant) -> Option<Page> {
        self.prune(now);
        let position = self.entries.iter().position(|entry| entry.key == *key)?;
        let entry = self.entries.remove(position).expect("cached search exists");
        let page = entry.page.clone();
        self.entries.push_back(entry);
        Some(page)
    }

    pub(super) fn insert(&mut self, key: Key, page: Page, now: Instant) {
        self.prune(now);
        self.entries.retain(|entry| entry.key != key);
        if self.entries.len() == CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            key,
            page,
            created: now,
        });
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
    }

    fn prune(&mut self, now: Instant) {
        self.entries
            .retain(|entry| now.saturating_duration_since(entry.created) < LIFETIME);
    }
}

#[cfg(test)]
mod tests;
