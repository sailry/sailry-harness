//! Kit layout snapshots are used only while arranging the current in-memory workspace.
use super::*;

pub(super) fn target(state: &PanelState) -> Option<Target> {
    let PanelInfo::Panel(value) = &state.info else {
        return None;
    };
    serde_json::from_value(value.clone()).ok()
}

pub(super) fn replace(state: &mut PanelState, from: Target, to: Target) {
    if target(state) == Some(from) {
        state.info = PanelInfo::panel(serde_json::to_value(to).unwrap());
    }
    for child in &mut state.children {
        replace(child, from, to);
    }
}

pub(super) fn swap(state: &PanelState, from: Target, to: Target) -> PanelState {
    let mut state = state.clone();
    fn visit(state: &mut PanelState, from: Target, to: Target) {
        match target(state) {
            Some(key) if key == from => {
                state.info = PanelInfo::panel(serde_json::to_value(to).unwrap())
            }
            Some(key) if key == to => {
                state.info = PanelInfo::panel(serde_json::to_value(from).unwrap())
            }
            _ => {}
        }
        for child in &mut state.children {
            visit(child, from, to);
        }
    }
    visit(&mut state, from, to);
    state
}

pub(super) fn layout(
    state: &PanelState,
    panes: &BTreeMap<Target, Entity<Pane>>,
) -> Option<DockLayout> {
    if let Some(target) = target(state) {
        return Some(DockLayout::tabs().panel(panes.get(&target)?.clone()));
    }
    let children: Vec<_> = state
        .children
        .iter()
        .enumerate()
        .filter_map(|(index, child)| {
            Some((
                layout(child, panes)?,
                state
                    .info
                    .sizes()
                    .and_then(|sizes| sizes.get(index))
                    .copied(),
            ))
        })
        .collect();
    if children.is_empty() {
        return None;
    }
    if children.len() == 1 {
        return children.into_iter().next().map(|(layout, _)| layout);
    }
    let mut split = if state.info.axis() == Some(Axis::Vertical) {
        DockLayout::v_split()
    } else {
        DockLayout::h_split()
    };
    for (child, size) in children {
        split = split.child(child, size);
    }
    Some(split)
}

impl Workspaces {
    pub fn hydrate(
        &mut self,
        target: Target,
        title: SharedString,
        project: SharedString,
        content: AnyView,
        cx: &mut Context<Self>,
    ) {
        if let Some(pane) = self.panes.get(&target) {
            pane.update(cx, |pane, cx| {
                pane.title = title;
                pane.project = project;
                pane.content = content;
                cx.notify();
            });
        }
    }
}
