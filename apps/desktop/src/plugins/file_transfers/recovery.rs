//! Core recovery participates in the same editor and transfer mutation locks.
use super::*;

pub(crate) fn reserve(
    location: Location,
    id: RequestId,
    path: &str,
    cx: &mut App,
) -> Result<(), &'static str> {
    registry(cx).update(cx, |state, cx| {
        if state
            .recovery
            .get(&id)
            .is_some_and(|held| held == &(location, path.into()))
        {
            return Ok(());
        }
        if state.affects(location, path) {
            return Err("files_operation_busy");
        }
        if state.dirty(location, path, cx) {
            return Err("checkpoint_unsaved");
        }
        state.recovery.insert(id, (location, path.into()));
        state.changed(cx);
        Ok(())
    })
}

pub(crate) fn release(id: RequestId, uncertain: bool, cx: &mut App) {
    if !uncertain {
        registry(cx).update(cx, |state, cx| {
            state.recovery.remove(&id);
            state.changed(cx);
        });
    }
}
