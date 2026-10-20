//! Presentation batches share one viewport; remote cursors still belong to each source.
use super::*;
use std::collections::HashSet;

const BATCH: usize = 12;

pub(super) struct State {
    pub limit: usize,
    pub total: usize,
    pub failed: Option<u32>,
    generation: u64,
    paused: bool,
    previous: Option<usize>,
    complete: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            limit: BATCH,
            total: 0,
            failed: None,
            generation: 0,
            paused: false,
            previous: None,
            complete: false,
        }
    }
}

impl State {
    pub fn reset(&mut self) {
        let generation = self.generation.wrapping_add(1);
        *self = Self::default();
        self.generation = generation;
    }
    fn accepts(&self, generation: u64, pending: bool, explicit: bool) -> bool {
        generation == self.generation
            && !pending
            && self.failed.is_none()
            && (!self.paused || explicit)
    }
    pub fn count(&mut self, total: usize) {
        if self.complete {
            self.complete = false;
            if let Some(previous) = self.previous.take() {
                self.paused = total <= previous;
            }
        }
        self.total = total;
    }
    pub fn reveal(&mut self) -> bool {
        if self.limit >= self.total {
            return false;
        }
        self.limit = self.limit.saturating_add(BATCH);
        true
    }
    pub fn received(&mut self, page: u32) {
        self.failed = None;
        if page > 1 {
            self.complete = true;
            self.limit = self.limit.saturating_add(BATCH);
        }
    }
}

pub(super) fn append<T>(stored: &mut Vec<T>, incoming: Vec<T>, id: impl Fn(&T) -> &str) -> usize {
    let mut known: HashSet<String> = stored.iter().map(|entry| id(entry).to_owned()).collect();
    let previous = stored.len();
    stored.extend(
        incoming
            .into_iter()
            .filter(|entry| known.insert(id(entry).to_owned())),
    );
    stored.len() - previous
}

fn advancing(current: u32, next: Option<u32>) -> Option<u32> {
    next.filter(|next| *next > current)
}

impl Market {
    pub(in crate::settings) fn viewport(&self) -> ScrollHandle {
        self.scroll.clone()
    }

    fn next_page(&self) -> Option<u32> {
        match self.kind {
            Kind::Plugins if self.source == Source::ThirdParty => self
                .page
                .as_ref()
                .filter(|page| page.page < page.pages)
                .map(|page| page.page + 1),
            Kind::Skills => self
                .skills
                .as_ref()
                .and_then(|page| advancing(page.page, page.next)),
            _ => None,
        }
    }

    fn more(&mut self, generation: u64, explicit: bool, cx: &mut Context<Self>) {
        if !self
            .paging
            .accepts(generation, self.pending.is_some(), explicit)
        {
            return;
        }
        if self.paging.reveal() {
            cx.notify();
            return;
        }
        let Some(page) = self.next_page() else {
            return;
        };
        self.paging.previous = Some(self.paging.total);
        self.paging.paused = false;
        match self.kind {
            Kind::Plugins => self.load_plugins(page, false, cx),
            Kind::Skills => self.load_skills(page, false, cx),
            Kind::Themes => {}
        }
    }

    pub(super) fn footer(&self, cx: &Context<Self>) -> AnyElement {
        let owner = cx.weak_entity();
        let scroll = self.scroll.clone();
        let generation = self.paging.generation;
        v_flex()
            .w_full()
            .items_center()
            .gap_2()
            .when(self.paging.paused && self.next_page().is_some(), |view| {
                view.child(
                    Button::new("market-load-more")
                        .ghost()
                        .small()
                        .label(tr("market_load_more"))
                        .disabled(self.pending.is_some())
                        .on_click(cx.listener(|state, _, _, cx| {
                            state.more(state.paging.generation, true, cx)
                        })),
                )
            })
            .when(
                self.pending == Some(Loading::Catalog) && self.paging.previous.is_some(),
                |view| view.child(spinner::Spinner::new().small()),
            )
            .child(div().h(px(1.)).w_full().on_prepaint(move |bounds, _, cx| {
                let viewport = scroll.bounds();
                if viewport.size.height > px(0.) && bounds.top() <= viewport.bottom() + px(96.) {
                    let owner = owner.clone();
                    cx.defer(move |cx| {
                        _ = owner.update(cx, |state, cx| state.more(generation, false, cx));
                    });
                }
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn reveals_filtered_batches_before_fetching() {
        let mut state = State {
            total: 29,
            ..Default::default()
        };
        assert_eq!(state.limit, 12);
        assert!(state.reveal());
        assert_eq!(state.limit, 24);
        assert!(state.reveal());
        assert!(!state.reveal());
        state.reset();
        assert_eq!(state.limit, 12);
        assert_eq!(state.total, 0);
        assert_eq!(state.generation, 1);
        assert!(!state.accepts(0, false, false));
        assert!(!state.accepts(1, true, true));
        state.previous = Some(0);
        state.count(0);
        assert!(!state.paused);
        state.received(2);
        state.count(0);
        assert!(state.paused);
        assert!(!state.accepts(1, false, false));
        assert!(state.accepts(1, false, true));
        state.previous = Some(0);
        state.received(3);
        state.count(1);
        assert!(!state.paused);
    }

    #[test]
    fn appends_stable_ids_without_reordering() {
        let mut rows = vec!["a".to_owned(), "b".to_owned()];
        assert_eq!(
            append(
                &mut rows,
                vec!["b".into(), "c".into(), "c".into()],
                String::as_str
            ),
            1
        );
        assert_eq!(rows, ["a", "b", "c"]);
        assert_eq!(append(&mut rows, vec!["a".into()], String::as_str), 0);
        let mut state = State {
            failed: Some(3),
            total: 24,
            ..Default::default()
        };
        assert_eq!(state.failed, Some(3));
        state.received(3);
        assert_eq!(state.failed, None);
        assert_eq!(state.limit, 24);
        state.failed = Some(4);
        state.reset();
        assert_eq!(state.failed, None);
        assert_eq!(advancing(2, Some(2)), None);
        assert_eq!(advancing(2, Some(1)), None);
        assert_eq!(advancing(2, Some(3)), Some(3));
        assert_eq!(advancing(2, None), None);
    }
}
