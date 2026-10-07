//! The current turn's activity and sources reuse the owning conversation projection.
use super::*;
use crate::conversation::subagent::Row;
use sailry_protocol::conversation::{Citation, RunKind, progress::Progress};
use std::collections::BTreeSet;

#[derive(Default)]
pub(super) struct Overview {
    pub turn: Option<TurnId>,
    pub children: Vec<Row>,
    pub progress: Option<Progress>,
    pub sources: Vec<Citation>,
}

impl Overview {
    pub fn new(view: &View) -> Self {
        let Some(snapshot) = &view.history.snapshot else {
            return Self::default();
        };
        let turn = view.active().or_else(|| {
            snapshot
                .page
                .runs
                .iter()
                .filter(|run| run.kind == RunKind::Task && run.status != Status::Queued)
                .max_by_key(|run| run.sequence)
                .map(|run| run.turn)
        });
        let Some(turn) = turn else {
            return Self::default();
        };
        let progress = view.turn_progress(turn);
        let mut seen = BTreeSet::new();
        let sources = snapshot
            .page
            .entries
            .iter()
            .filter(|entry| entry.turn == turn)
            .flat_map(|entry| &entry.citations)
            .filter(|source| seen.insert(source.uri.clone()))
            .cloned()
            .collect();
        Self {
            turn: Some(turn),
            children: view
                .child_activity_turn()
                .map(|turn| view.child_rows(turn))
                .unwrap_or_default(),
            progress,
            sources,
        }
    }
}
