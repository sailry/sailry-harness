//! Kit layout snapshots are also the desktop persistence format.
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

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Saved {
    pub active: Option<Target>,
    pub groups: Vec<PanelState>,
    pub parents: Vec<(Target, Target)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<(Target, Extension)>,
}
impl Saved {
    pub fn validate(&self) -> Result<(), String> {
        fn visit(
            state: &PanelState,
            seen: &mut std::collections::BTreeSet<Target>,
        ) -> Result<usize, String> {
            match &state.info {
                PanelInfo::Panel(_) => {
                    let Some(key @ (Target::Session(..) | Target::Terminal(..))) = target(state)
                    else {
                        return Err("workspace contains an invalid resource".into());
                    };
                    if !state.children.is_empty() || !seen.insert(key) {
                        return Err("workspace contains a duplicate resource".into());
                    }
                    Ok(1)
                }
                PanelInfo::Stack { sizes, axis } => {
                    if *axis > 1
                        || sizes.len() != state.children.len()
                        || sizes
                            .iter()
                            .any(|size| !f32::from(*size).is_finite() || *size < px(0.))
                    {
                        return Err("invalid workspace split dimensions".into());
                    }
                    state
                        .children
                        .iter()
                        .try_fold(0, |count, child| Ok(count + visit(child, seen)?))
                }
                PanelInfo::Tabs { active_index: 0 } if state.children.len() == 1 => {
                    visit(&state.children[0], seen)
                }
                _ => Err("invalid workspace container".into()),
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut memberships = BTreeMap::new();
        for (index, group) in self.groups.iter().enumerate() {
            let before = seen.clone();
            let count = visit(group, &mut seen)?;
            memberships.extend(seen.difference(&before).map(|target| (*target, index)));
            if count == 0 || count > LIMIT {
                return Err("workspace exceeds the pane limit".into());
            }
        }
        if self.active.is_some_and(|target| !seen.contains(&target)) {
            return Err("active workspace resource is missing".into());
        }
        let mut renderers = std::collections::BTreeSet::new();
        for (target, extension) in &self.extensions {
            if !matches!(target, Target::Terminal(..))
                || !seen.contains(target)
                || !renderers.insert(*target)
                || extension.package.is_empty()
                || extension.package.len() > 128
                || extension.package.chars().any(char::is_control)
            {
                return Err("invalid workspace renderer".into());
            }
        }
        let parents: BTreeMap<_, _> = self.parents.iter().copied().collect();
        if parents.len() != self.parents.len() {
            return Err("workspace contains duplicate parents".into());
        }
        for (child, parent) in &parents {
            if !seen.contains(child)
                || !seen.contains(parent)
                || memberships[child] != memberships[parent]
            {
                return Err("workspace parent is outside its split group".into());
            }
            let mut visited = std::collections::BTreeSet::from([*child]);
            let mut current = Some(*parent);
            while let Some(target) = current {
                if !visited.insert(target) {
                    return Err("workspace contains cyclic parents".into());
                }
                current = parents.get(&target).copied();
            }
        }
        Ok(())
    }
    pub fn targets(&self) -> Vec<Target> {
        fn collect(state: &PanelState, targets: &mut Vec<Target>) {
            if let Some(target @ (Target::Session(..) | Target::Terminal(..))) = target(state) {
                targets.push(target);
            }
            for child in &state.children {
                collect(child, targets);
            }
        }
        let mut targets = Vec::new();
        for group in &self.groups {
            collect(group, &mut targets);
        }
        targets.sort();
        targets.dedup();
        targets
    }
}
fn resources(state: &PanelState) -> Option<PanelState> {
    if let Some(target) = target(state) {
        return (target != Target::Draft).then(|| state.clone());
    }
    let mut result = state.clone();
    result.children.clear();
    let mut kept_sizes = Vec::new();
    for (index, child) in state.children.iter().enumerate() {
        if let Some(child) = resources(child) {
            result.children.push(child);
            if let Some(size) = state.info.sizes().and_then(|sizes| sizes.get(index)) {
                kept_sizes.push(*size);
            }
        }
    }
    if let PanelInfo::Stack { sizes, .. } = &mut result.info {
        *sizes = kept_sizes;
    }
    if result.children.len() == 1 {
        return result.children.pop();
    }
    (!result.children.is_empty()).then_some(result)
}
impl Workspaces {
    pub fn saved(&self, cx: &App) -> Saved {
        let mut hierarchy = hierarchy::Hierarchy(self.hierarchy.0.clone());
        hierarchy.remove(Target::Draft);
        Saved {
            active: self.active.filter(|target| *target != Target::Draft),
            groups: self
                .groups
                .values()
                .filter_map(|group| resources(&group.area.read(cx).dump(cx).center))
                .collect(),
            parents: hierarchy.0.into_iter().collect(),
            extensions: self
                .panes
                .iter()
                .filter_map(|(target, pane)| Some((*target, pane.read(cx).extension.clone()?)))
                .collect(),
        }
    }
    pub fn restore(&mut self, saved: &Saved, window: &mut Window, cx: &mut Context<Self>) {
        for target in saved.targets() {
            let content = cx.new(|_| Empty);
            self.open(
                target,
                crate::tr("chat_new"),
                "".into(),
                content.into(),
                window,
                cx,
            );
        }
        for (target, extension) in &saved.extensions {
            self.panes[target].update(cx, |pane, _| pane.extension = Some(extension.clone()));
        }
        self.groups.clear();
        self.hierarchy.0 = saved.parents.iter().copied().collect();
        for group in &saved.groups {
            let Some(layout) = layout(group, &self.panes) else {
                continue;
            };
            let Some(first) = self.panes.values().next().cloned() else {
                continue;
            };
            self.standalone(first, window, cx);
            let area = self.groups[&(self.next - 1)].area.clone();
            area.update(cx, |area, cx| area.set_center(layout, window, cx));
        }
        self.active = saved
            .active
            .filter(|target| self.contains(*target))
            .or_else(|| self.panes.keys().next().copied());
    }
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
