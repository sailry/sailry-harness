//! Recovery guards belong to one admitted Node task and its ADK invocation.
use super::*;
use crate::agent::model::history::Masks;

#[derive(Default)]
pub(in crate::agent) struct State {
    tokens: Mutex<Option<u64>>,
    overflow: AtomicBool,
    masked: Mutex<Masks>,
    projection: Mutex<Option<String>>,
}

impl State {
    /// After an attempted summary, another waits for 10% context growth.
    pub(super) fn allows(&self, tokens: u64, forced: bool) -> adk_core::Result<bool> {
        let previous = self
            .tokens
            .lock()
            .map_err(|_| invalid("context recovery state is unavailable"))?;
        Ok(forced
            || previous.is_none_or(|previous| tokens > previous.saturating_add(previous / 10)))
    }

    pub(super) fn record(&self, tokens: u64) -> adk_core::Result<()> {
        *self
            .tokens
            .lock()
            .map_err(|_| invalid("context recovery state is unavailable"))? = Some(tokens);
        Ok(())
    }

    pub(super) fn overflow(&self) -> bool {
        !self.overflow.swap(true, Ordering::AcqRel)
    }

    pub(super) fn masked(&self) -> adk_core::Result<Masks> {
        Ok(self
            .masked
            .lock()
            .map_err(|_| invalid("context recovery state is unavailable"))?
            .clone())
    }

    pub(super) fn mask(&self, calls: Masks) -> adk_core::Result<()> {
        *self
            .masked
            .lock()
            .map_err(|_| invalid("context recovery state is unavailable"))? = calls;
        Ok(())
    }

    /// Returns whether a window projection differs from the last one saved.
    pub(super) fn project(&self, fingerprint: Option<String>) -> adk_core::Result<bool> {
        let mut previous = self
            .projection
            .lock()
            .map_err(|_| invalid("context recovery state is unavailable"))?;
        let Some(fingerprint) = fingerprint else {
            *previous = None;
            return Ok(false);
        };
        if previous.as_ref() == Some(&fingerprint) {
            return Ok(false);
        }
        *previous = Some(fingerprint);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suppresses_small_growth() {
        let state = State::default();
        assert!(state.allows(1000, false).unwrap());
        state.record(1000).unwrap();
        assert!(!state.allows(1000, false).unwrap());
        assert!(!state.allows(1100, false).unwrap());
        assert!(state.allows(1101, false).unwrap());
        assert!(state.allows(0, true).unwrap());
        state.record(400).unwrap();
        assert!(!state.allows(440, false).unwrap());
        assert!(state.allows(441, false).unwrap());
    }

    #[test]
    fn forces_one_overflow_recovery() {
        let state = State::default();
        assert!(state.overflow());
        state.record(1000).unwrap();
        assert!(!state.overflow());
    }

    #[test]
    fn saves_only_changed_projections() {
        let state = State::default();
        assert!(!state.project(None).unwrap());
        assert!(state.project(Some("first".into())).unwrap());
        assert!(!state.project(Some("first".into())).unwrap());
        assert!(state.project(Some("second".into())).unwrap());
        assert!(!state.project(None).unwrap());
        assert!(state.project(Some("second".into())).unwrap());
        state
            .mask(Masks::from([("call".into(), "revision".into())]))
            .unwrap();
        assert!(
            state
                .masked()
                .unwrap()
                .contains(&("call".into(), "revision".into()))
        );
    }
}
