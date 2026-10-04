//! Controller-owned ordering preferences, scoped to a Node/session pair.
use super::*;
use sailry_protocol::{NodeId, SessionId};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct State {
    pub pinned: bool,
}
fn key(node: NodeId, session: SessionId) -> String {
    let node: String = node.0.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{node}/{session}")
}
pub(crate) fn get(node: NodeId, session: SessionId, cx: &App) -> State {
    cx.try_global::<Preferences>()
        .and_then(|p| p.data.sessions.as_ref()?.get(&key(node, session)))
        .copied()
        .unwrap_or_default()
}
pub(crate) fn set(node: NodeId, session: SessionId, state: State, cx: &mut App) {
    if get(node, session, cx) == state {
        return;
    }
    super::update(cx, |data| {
        let sessions = data.sessions.get_or_insert_default();
        if state == State::default() {
            sessions.remove(&key(node, session));
        } else {
            sessions.insert(key(node, session), state);
        }
    });
}
