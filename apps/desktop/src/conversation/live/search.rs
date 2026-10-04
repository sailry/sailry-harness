//! Conversation-scoped search composition; retrieval and turn loading stay in Client.
use super::*;
use gpui_kit::component::list::{ListEvent, ListState};
use sailry_protocol::conversation::search::Match;

mod render;
mod results;
mod reveal;
#[cfg(test)]
mod tests;
use results::Results;

pub(super) enum Event {
    Reveal(Match),
    Reset,
}

pub(crate) struct Search {
    list: Entity<ListState<Results>>,
    open: bool,
    query_generation: u64,
}

impl EventEmitter<Event> for Search {}

impl Search {
    pub(super) fn new(
        binding: Binding,
        session: Option<SessionId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let list = cx
            .new(|cx| ListState::new(Results::new(binding, session), window, cx).searchable(true));
        let owner = cx.weak_entity();
        list.update(cx, |_, cx| Results::observe_errors(owner, window, cx));
        cx.subscribe_in(
            &list,
            window,
            |search, list, event, window, cx| match event {
                ListEvent::Confirm(index) => {
                    if let Some(target) = list.read(cx).delegate().matches.get(index.row).cloned() {
                        search.set_open(false, window, cx);
                        cx.emit(Event::Reveal(target));
                    }
                }
                ListEvent::Cancel => search.set_open(false, window, cx),
                ListEvent::Select(_) => {}
            },
        )
        .detach();
        cx.observe(&list, |search, list, cx| {
            let generation = list.read(cx).delegate().query_generation;
            if search.query_generation != generation {
                search.query_generation = generation;
                cx.emit(Event::Reset);
            }
            cx.notify();
        })
        .detach();
        Self {
            list,
            open: false,
            query_generation: 0,
        }
    }

    pub(super) fn sync(
        &mut self,
        session: Option<SessionId>,
        revision: Option<u64>,
        connected: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.list.read(cx).delegate().session != session
            || self.list.read(cx).delegate().revision != revision
        {
            self.list.update(cx, |list, cx| {
                let results = list.delegate_mut();
                results.cancel();
                results.session = session;
                results.revision = revision;
                results.query_generation += 1;
                results.matches.clear();
                results.before = None;
                results.error = None;
                cx.notify();
            });
            if self.open && connected {
                self.refresh(window, cx);
            }
        }
    }

    pub(super) fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.list.read(cx).delegate().session.is_some() {
            if self.open {
                self.list.update(cx, |list, cx| list.focus(window, cx));
            } else {
                self.set_open(true, window, cx);
            }
        }
    }

    fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.open == open {
            return;
        }
        self.open = open;
        if open {
            cx.emit(Event::Reset);
            self.refresh(window, cx);
            self.show(window, cx);
        } else {
            window.close_dialog(cx);
            self.dismiss(cx);
        }
        cx.notify();
    }

    fn show(&self, window: &mut Window, cx: &mut Context<Self>) {
        let owner = cx.entity();
        window.open_dialog(cx, move |dialog, window, cx| {
            owner.update(cx, |search, cx| search.dialog(dialog, window, cx))
        });
        let list = self.list.clone();
        window.defer(cx, move |window, cx| {
            list.update(cx, |list, cx| list.focus(window, cx))
        });
    }

    fn retry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            self.open = true;
            self.show(window, cx);
        }
        self.list.update(cx, |list, cx| {
            let results = list.delegate_mut();
            results.more = Some(results.request(results.before, window, cx));
            list.focus(window, cx);
        });
        cx.notify();
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.list.update(cx, |list, cx| {
            list.delegate_mut().cancel();
            cx.notify();
        });
        cx.notify();
    }

    fn refresh(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            let query = list.delegate().query.clone();
            list.set_query(&query, window, cx);
        });
    }
}
