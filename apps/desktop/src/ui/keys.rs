//! Binding ownership without replacing GPUI's focus or action dispatch.
use gpui_kit::*;
use std::sync::atomic::{AtomicU32, Ordering};

pub(crate) struct Bindings {
    meta: KeyBindingMetaIndex,
    bindings: Vec<KeyBinding>,
}

impl Bindings {
    pub(crate) fn new(cx: &mut App) -> Entity<Self> {
        static NEXT: AtomicU32 = AtomicU32::new(0x8000_0000);
        let state = cx.new(|_| Self {
            meta: KeyBindingMetaIndex(NEXT.fetch_add(1, Ordering::Relaxed)),
            bindings: Vec::new(),
        });
        cx.observe_release(&state, |state, cx| state.replace(Vec::new(), cx))
            .detach();
        state
    }

    pub(crate) fn context(&self) -> String {
        format!("SailryKeys{}", self.meta.0)
    }

    pub(crate) fn replace(&mut self, bindings: Vec<KeyBinding>, cx: &mut App) {
        if self.bindings.len() == bindings.len()
            && self.bindings.iter().zip(&bindings).all(|(left, right)| {
                left.keystrokes() == right.keystrokes()
                    && left.predicate() == right.predicate()
                    && left.action().partial_eq(right.action())
            })
        {
            return;
        }
        let retained = cx
            .key_bindings()
            .borrow()
            .bindings()
            .filter(|binding| binding.meta() != Some(self.meta))
            .cloned()
            .collect::<Vec<_>>();
        cx.clear_key_bindings();
        cx.bind_keys(retained);
        cx.bind_keys(
            bindings
                .iter()
                .cloned()
                .map(|binding| binding.with_meta(self.meta)),
        );
        self.bindings = bindings;
    }
}

#[cfg(test)]
mod tests;
