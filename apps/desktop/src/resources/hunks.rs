use crate::{
    conversation::live::{Binding, View},
    tr,
};
use gpui_kit::{
    component::{WindowExt, button::Button, notification::Notification},
    *,
};
use sailry_protocol::{
    Command, ErrorCode, Output, Request, SessionId, TurnId, conversation::checkpoint::hunk,
};
use std::collections::BTreeMap;

mod render;
mod requests;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Decision {
    Accepted,
    Rejected,
    Staged,
}

pub(crate) struct Review {
    source: WeakEntity<View>,
    binding: Binding,
    session: SessionId,
    turn: TurnId,
    path: String,
    data: Option<hunk::Review>,
    revision: Option<String>,
    index: Option<String>,
    head: Option<String>,
    decisions: BTreeMap<String, Decision>,
    request: Option<Request>,
    pending: bool,
    failed: bool,
}

impl Review {
    pub(crate) fn new(
        source: WeakEntity<View>,
        binding: Binding,
        session: SessionId,
        turn: TurnId,
        path: String,
    ) -> Self {
        Self {
            source,
            binding,
            session,
            turn,
            path,
            data: None,
            revision: None,
            index: None,
            head: None,
            decisions: BTreeMap::new(),
            request: None,
            pending: false,
            failed: false,
        }
    }

    pub(crate) fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let owner = cx.entity();
        window.open_dialog(cx, move |dialog, window, cx| {
            owner.update(cx, |review, cx| review.dialog(dialog, window, cx))
        });
        if self.data.is_none() && !self.pending {
            self.load(window, cx);
        }
    }

    fn connected(&self, cx: &App) -> bool {
        self.source
            .read_with(cx, |view, _| {
                view.connected() && view.session() == Some(self.session)
            })
            .unwrap_or(false)
    }

    fn accept(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.request.is_some() || !self.connected(cx) {
            return;
        }
        if !self
            .data
            .as_ref()
            .is_some_and(|data| data.hunks.iter().any(|hunk| hunk.id == id))
        {
            return;
        }
        self.decisions.insert(id.to_owned(), Decision::Accepted);
        crate::feedback::toast(
            window,
            tr("hunks_accepted"),
            Notification::success(tr("hunks_accepted")),
            cx,
        );
        cx.notify();
    }

    fn report(&self, issue: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        crate::feedback::toast(window, tr(issue), Notification::error(tr(issue)), cx);
    }
}
