//! Controller-owned ordering preferences, scoped to a Node/session pair.
use super::*;
use sailry_protocol::{NodeId, SessionId};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct State {
    pub pinned: bool,
    #[serde(default)]
    pub order: Option<u64>,
}

#[cfg(test)]
mod tests;
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
pub(crate) fn set(node: NodeId, session: SessionId, mut state: State, cx: &mut App) {
    if !state.pinned {
        state = State::default();
    }
    if get(node, session, cx) == state {
        return;
    }
    super::update(cx, |data| {
        let sessions = data.sessions.get_or_insert_default();
        let key = key(node, session);
        if state == State::default() {
            sessions.remove(&key);
        } else {
            if state.order.is_none() {
                let prefix = key.rsplit_once('/').unwrap().0;
                state.order = Some(
                    sessions
                        .iter()
                        .filter(|(key, state)| {
                            state.pinned
                                && key
                                    .rsplit_once('/')
                                    .is_some_and(|(owner, _)| owner == prefix)
                        })
                        .filter_map(|(_, state)| state.order)
                        .max()
                        .map_or(0, |order| order.saturating_add(1)),
                );
            }
            sessions.insert(key, state);
        }
    });
}

pub(crate) fn reconcile(view: &sailry_client::View, cx: &mut App) {
    let Some(snapshot) = view
        .snapshot
        .as_ref()
        .filter(|_| view.connected && view.error.is_none())
    else {
        return;
    };
    super::init(cx);
    let cursor = cx
        .global_mut::<Preferences>()
        .session_cursors
        .entry(snapshot.node)
        .or_default();
    if *cursor > snapshot.cursor {
        return;
    }
    *cursor = snapshot.cursor;
    let retained: std::collections::BTreeSet<_> = snapshot
        .sessions
        .iter()
        .filter(|session| !session.archived)
        .map(|session| key(snapshot.node, session.id))
        .collect();
    let prefix: String = snapshot
        .node
        .0
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let prefix = format!("{prefix}/");
    let keep = |key: &String| !key.starts_with(&prefix) || retained.contains(key);
    if cx
        .try_global::<Preferences>()
        .and_then(|preferences| preferences.data.sessions.as_ref())
        .is_none_or(|sessions| sessions.keys().all(&keep))
    {
        return;
    }
    super::update(cx, |data| {
        if let Some(sessions) = &mut data.sessions {
            sessions.retain(|key, _| keep(key));
        }
    });
}
