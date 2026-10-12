use super::*;
use sailry_link::CancellationToken;
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct State {
    entries: BTreeMap<Target, Attempt>,
    generation: u64,
    pub(super) stop: CancellationToken,
}

struct Attempt {
    generation: u64,
    stop: CancellationToken,
    pub(super) view: Entity<View>,
}

impl Drop for State {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl State {
    pub(crate) fn contains(&self, target: Target) -> bool {
        self.entries.contains_key(&target)
    }

    pub(crate) fn pending(&self, target: Target, cx: &App) -> bool {
        self.entries
            .get(&target)
            .is_some_and(|entry| entry.view.read(cx).pending)
    }

    pub(super) fn begin(&mut self, target: Target, view: Entity<View>) -> u64 {
        self.cancel(target);
        self.generation = self
            .generation
            .checked_add(1)
            .expect("recovery generation exhausted");
        self.entries.insert(
            target,
            Attempt {
                generation: self.generation,
                stop: self.stop.child_token(),
                view,
            },
        );
        self.generation
    }

    pub(super) fn current(&self, target: Target, generation: u64) -> bool {
        self.entries
            .get(&target)
            .is_some_and(|entry| entry.generation == generation && !entry.stop.is_cancelled())
    }

    pub(super) fn fail(&self, target: Target, error: sailry_protocol::Fault, cx: &mut App) {
        if let Some(entry) = self.entries.get(&target) {
            entry.view.update(cx, |view, cx| view.failed(error, cx));
        }
    }

    pub(crate) fn cancel(&mut self, target: Target) {
        if let Some(entry) = self.entries.remove(&target) {
            entry.stop.cancel();
        }
    }
}
