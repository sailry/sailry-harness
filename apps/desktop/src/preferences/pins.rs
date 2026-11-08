//! Controller-owned pins and ordering, scoped to a Node/resource pair.
use super::*;
use sailry_protocol::{NodeId, SessionId, TerminalId, terminal::Status};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Session(SessionId),
    Terminal(TerminalId),
}

impl From<SessionId> for Target {
    fn from(id: SessionId) -> Self {
        Self::Session(id)
    }
}

impl From<TerminalId> for Target {
    fn from(id: TerminalId) -> Self {
        Self::Terminal(id)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct State {
    pub pinned: bool,
    #[serde(default)]
    pub order: Option<u64>,
}

#[cfg(test)]
mod tests;
fn prefix(node: NodeId) -> String {
    let node: String = node.0.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{node}/")
}
fn key(node: NodeId, target: impl Into<Target>) -> String {
    let prefix = prefix(node);
    match target.into() {
        Target::Session(id) => format!("{prefix}{id}"),
        Target::Terminal(id) => format!("{prefix}terminal/{id}"),
    }
}
pub(crate) fn get(node: NodeId, target: impl Into<Target>, cx: &App) -> State {
    cx.try_global::<Preferences>()
        .and_then(|p| p.data.pins.as_ref()?.get(&key(node, target)))
        .copied()
        .unwrap_or_default()
}
pub(crate) fn set(node: NodeId, target: impl Into<Target>, mut state: State, cx: &mut App) {
    let target = target.into();
    if !state.pinned {
        state = State::default();
    }
    if get(node, target, cx) == state {
        return;
    }
    super::update(cx, |data| {
        let pins = data.pins.get_or_insert_default();
        let key = key(node, target);
        if state == State::default() {
            pins.remove(&key);
        } else {
            if state.order.is_none() {
                let prefix = prefix(node);
                state.order = Some(
                    pins.iter()
                        .filter(|(key, state)| state.pinned && key.starts_with(&prefix))
                        .filter_map(|(_, state)| state.order)
                        .max()
                        .map_or(0, |order| order.saturating_add(1)),
                );
            }
            pins.insert(key, state);
        }
    });
}

pub(crate) fn retain_hosts(nodes: impl IntoIterator<Item = NodeId>, cx: &mut App) {
    let nodes = nodes.into_iter().collect::<std::collections::BTreeSet<_>>();
    let prefixes = nodes.iter().copied().map(prefix).collect::<Vec<_>>();
    let keep = |key: &String| prefixes.iter().any(|prefix| key.starts_with(prefix));
    super::init(cx);
    let preferences = cx.global_mut::<Preferences>();
    preferences
        .pin_cursors
        .retain(|node, _| nodes.contains(node));
    if preferences
        .data
        .pins
        .as_ref()
        .is_none_or(|pins| pins.keys().all(&keep))
    {
        return;
    }
    super::update(cx, |data| {
        if let Some(pins) = &mut data.pins {
            pins.retain(|key, _| keep(key));
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
        .pin_cursors
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
        .chain(
            snapshot
                .terminals
                .iter()
                .filter(|terminal| terminal.status != Status::Closed)
                .map(|terminal| key(snapshot.node, terminal.id)),
        )
        .collect();
    let prefix = prefix(snapshot.node);
    let keep = |key: &String| !key.starts_with(&prefix) || retained.contains(key);
    if cx
        .try_global::<Preferences>()
        .and_then(|preferences| preferences.data.pins.as_ref())
        .is_none_or(|pins| pins.keys().all(&keep))
    {
        return;
    }
    super::update(cx, |data| {
        if let Some(pins) = &mut data.pins {
            pins.retain(|key, _| keep(key));
        }
    });
}
