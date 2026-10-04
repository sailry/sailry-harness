//! Append to a split using Kit's current visual order and measured size.
use super::*;

impl Workspaces {
    pub fn ordered_members(&self, target: Target, cx: &App) -> Vec<Target> {
        fn visit(state: &PanelState, members: &mut Vec<Target>) {
            if let Some(target) = state::target(state) {
                members.push(target);
            }
            for child in &state.children {
                visit(child, members);
            }
        }
        let mut members = Vec::new();
        if let Some(workspace) = self.workspace(target, cx) {
            visit(
                &self.groups[&workspace].area.read(cx).dump(cx).center,
                &mut members,
            );
        }
        members
    }

    pub fn can_append(&self, destination: Target, target: Target, cx: &App) -> bool {
        let Some(workspace) = self.workspace(destination, cx) else {
            return false;
        };
        target != Target::Draft
            && self.workspace(target, cx) != Some(workspace)
            && (2..LIMIT).contains(&self.members(workspace, cx).len())
    }

    pub fn append(
        &mut self,
        destination: Target,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.can_append(destination, target, cx) {
            return false;
        }
        let Some(source) = self.workspace(target, cx) else {
            return false;
        };
        let workspace = self.workspace(destination, cx).unwrap();
        let area = self.groups[&workspace].area.clone();
        let mut members = self.ordered_members(destination, cx);
        members.push(target);
        let bounds = area.read(cx).bounds();
        let width = if bounds.size.width > px(0.) {
            bounds.size.width
        } else {
            window.viewport_size().width
        };
        let columns = columns(members.len(), width);
        let mut rows = DockLayout::v_split();
        for row in members.chunks(columns) {
            let mut layout = DockLayout::h_split();
            for member in row {
                layout = layout.child(DockLayout::tabs().panel(self.panes[member].clone()), None);
            }
            rows = rows.child(layout, None);
        }
        let pane = self.panes[&target].clone();
        self.groups[&source]
            .area
            .update(cx, |area, cx| area.remove_panel(pane, window, cx));
        self.hierarchy.attach(target, destination);
        self.saving = true;
        area.update(cx, |area, cx| area.set_center(rows, window, cx));
        self.prune(cx);
        self.focus(target, cx);
        cx.emit(Event::Changed);
        cx.notify();
        true
    }
}

fn columns(count: usize, width: Pixels) -> usize {
    // Keep conversation panes readable; add rows once the available width is used.
    const MIN_WIDTH: f32 = 320.;
    let available = (f32::from(width) / MIN_WIDTH).floor().max(1.) as usize;
    let balanced = (count as f32).sqrt().ceil() as usize;
    balanced.min(available).clamp(1, 3)
}
