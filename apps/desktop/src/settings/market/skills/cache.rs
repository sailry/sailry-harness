//! View snapshots retain pagination while the Node refreshes the same public query.
use gpui_kit::{Pixels, Point};
use sailry_protocol::{
    NodeId,
    plugin::skillcatalog::{Page, Source},
};

#[derive(Clone, PartialEq, Eq)]
pub(in crate::settings::market) struct Key {
    pub node: NodeId,
    pub source: Source,
    pub query: String,
    pub installed: bool,
}

#[derive(Clone)]
pub(in crate::settings::market) struct Snapshot {
    pub page: Page,
    pub limit: usize,
    pub offset: Point<Pixels>,
}

#[derive(Default)]
pub(in crate::settings::market) struct Cache(Vec<(Key, Snapshot)>);

impl Cache {
    pub fn remember(&mut self, key: Key, value: Snapshot) {
        self.0.retain(|(stored, _)| stored != &key);
        if self.0.len() == 12 {
            self.0.remove(0);
        }
        self.0.push((key, value));
    }

    pub fn get(&self, key: &Key) -> Option<Snapshot> {
        self.0
            .iter()
            .find(|(stored, _)| stored == key)
            .map(|(_, value)| value.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn separates_queries_and_keeps_page_state() {
        let mut cache = Cache::default();
        let key = Key {
            node: NodeId([1; 32]),
            source: Source::Skillhub,
            query: "word".into(),
            installed: false,
        };
        let snapshot = Snapshot {
            page: Page {
                query_scope: sailry_protocol::plugin::skillcatalog::QueryScope::Catalog,
                entries: vec![],
                page: 3,
                next: Some(4),
                total: Some(80),
                unavailable: false,
            },
            limit: 36,
            offset: gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(-120.)),
        };
        cache.remember(key.clone(), snapshot);
        let restored = cache.get(&key).unwrap();
        assert_eq!(restored.page.page, 3);
        assert_eq!(restored.page.next, Some(4));
        assert_eq!(restored.limit, 36);
        assert_eq!(restored.offset.y, gpui_kit::px(-120.));
        assert!(
            cache
                .get(&Key {
                    node: NodeId([2; 32]),
                    ..key.clone()
                })
                .is_none()
        );
        assert!(
            cache
                .get(&Key {
                    source: Source::SkillsSh,
                    ..key.clone()
                })
                .is_none()
        );
        assert!(
            cache
                .get(&Key {
                    installed: true,
                    ..key.clone()
                })
                .is_none()
        );
        assert!(
            cache
                .get(&Key {
                    query: "other".into(),
                    ..key
                })
                .is_none()
        );
    }
}
